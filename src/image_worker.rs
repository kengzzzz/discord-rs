//! Decodes untrusted images in a child process, so a decoder bomb kills only the worker.
//!
//! Request: `kind u8, deadline_ms u32, max_out u32, len u32, payload`.
//! Response: `status u8 (0 ok, 1 refused), partial u8, len u32, payload`.

use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use crate::{
    ai::gemini,
    config::ScamConfig,
    moderation::{ocr, scoring},
};

pub const ARG: &str = "image-worker";

const OCR: u8 = 1;
const CONVERT_GIF: u8 = 2;
const CONVERT_BMP: u8 = 3;
const LOAD: u8 = 4;
const MAX_REQUEST: usize = 64 << 20;
/// Extra time to spawn a worker and load the models.
const GRACE: Duration = Duration::from_secs(5);
/// Conversions share the worker with scans, so a slow file can't hold it long.
const CONVERT_RUN: Duration = Duration::from_secs(3);
const CONVERT_GRACE: Duration = Duration::from_secs(1);
const CONVERT_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("{0}")]
    Refused(String),
    #[error("image worker failed: {0}")]
    Crashed(String),
    #[error("image worker busy or the job's time is spent")]
    Late,
}

pub fn run() -> anyhow::Result<()> {
    let config = ScamConfig::from_env()?;
    // A bomb then fails an allocation here instead of waking the OOM killer, which could take
    // the bot too.
    let bytes = config
        .worker_memory_mb
        .saturating_mul(1 << 20);
    let limit = libc::rlimit { rlim_cur: bytes as libc::rlim_t, rlim_max: bytes as libc::rlim_t };
    // SAFETY: setrlimit reads one valid, initialised struct and changes only this process.
    if unsafe { libc::setrlimit(libc::RLIMIT_DATA, &limit) } != 0 {
        anyhow::bail!(
            "could not limit the image worker's memory: {}",
            std::io::Error::last_os_error()
        );
    }
    // Aborts at the cap are expected, so skip core dumps.
    let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: as above.
    unsafe { libc::setrlimit(libc::RLIMIT_CORE, &none) };
    // Backstop: if the OOM killer runs anyway, it picks this process.
    if let Err(error) = std::fs::write("/proc/self/oom_score_adj", "1000") {
        tracing::warn!(%error, "image worker could not raise its OOM score");
    }
    serve(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &config,
    )
}

fn serve(
    input: &mut impl Read,
    output: &mut impl Write,
    config: &ScamConfig,
) -> anyhow::Result<()> {
    let mut engine = None;
    loop {
        let mut head = [0u8; 13];
        match input.read_exact(&mut head) {
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            result => result?,
        }
        let field =
            |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        let deadline = Instant::now() + Duration::from_millis(u64::from(field(1)));
        let max_out = field(5) as usize;
        let len = field(9) as usize;
        anyhow::ensure!(len <= MAX_REQUEST, "request of {len} bytes");
        let mut payload = vec![0; len];
        input.read_exact(&mut payload)?;
        let result =
            handle(head[0], payload, deadline, config, &mut engine).and_then(|(out, partial)| {
                anyhow::ensure!(
                    out.len() <= max_out,
                    "result of {} bytes is over {max_out}",
                    out.len()
                );
                Ok((out, partial))
            });
        let (status, partial, body) = match result {
            Ok((out, partial)) => (0, partial, out),
            // The parent reads at most 4096 bytes of an error.
            Err(error) => {
                let message: String = format!("{error:#}")
                    .chars()
                    .take(1000)
                    .collect();
                (1, false, message.into_bytes())
            }
        };
        output.write_all(&[status, u8::from(partial)])?;
        output.write_all(&(body.len() as u32).to_le_bytes())?;
        output.write_all(&body)?;
        output.flush()?;
    }
}

fn handle(
    kind: u8,
    payload: Vec<u8>,
    deadline: Instant,
    config: &ScamConfig,
    engine: &mut Option<ocr::Engine>,
) -> anyhow::Result<(Vec<u8>, bool)> {
    match kind {
        LOAD => loaded(engine, config).map(|_| (Vec::new(), false)),
        OCR => {
            let engine = loaded(engine, config)?;
            // Transparent images show on light or dark, so read both or framing can hide text.
            let mut best: Option<(f32, String)> = None;
            // Any read cut by the deadline makes the result partial.
            let mut cut = false;
            let images = ocr::decode(&payload, config)?;
            let count = images.len() as u32;
            for (index, image) in images.iter().enumerate() {
                // Split the time left, so the dark read still runs.
                let left = deadline.saturating_duration_since(Instant::now());
                let share = deadline - left + left / (count - index as u32);
                let (text, partial) = match engine.read(image, config, share) {
                    Ok(read) => read,
                    // Keep what the first read found.
                    Err(error) if best.is_some() => {
                        tracing::warn!(%error, "second OCR read failed");
                        break;
                    }
                    Err(error) => return Err(error),
                };
                let risk = scoring::score_text(
                    &text,
                    config.block_threshold,
                    config.review_threshold,
                )
                .risk;
                if best
                    .as_ref()
                    .is_none_or(|(top, ..)| risk > *top)
                {
                    best = Some((risk, text));
                }
                if partial {
                    cut = true;
                }
            }
            let (_, text) = best.unwrap_or_default();
            Ok((text.into_bytes(), cut))
        }
        CONVERT_GIF | CONVERT_BMP => {
            let mime = if kind == CONVERT_GIF { "image/gif" } else { "image/bmp" };
            gemini::normalize_image(mime, payload)
                .map(|(_, png)| (png, false))
                .ok_or_else(|| anyhow::anyhow!("image could not be converted"))
        }
        other => anyhow::bail!("unknown job kind {other}"),
    }
}

fn loaded<'a>(
    engine: &'a mut Option<ocr::Engine>,
    config: &ScamConfig,
) -> anyhow::Result<&'a ocr::Engine> {
    if engine.is_none() {
        *engine = Some(ocr::Engine::new(config)?);
    }
    Ok(engine.as_ref().expect("loaded above"))
}

struct Running {
    // `kill_on_drop`, so a cancelled job never leaves half a request in the pipe.
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

pub struct ImageWorker {
    name: &'static str,
    running: tokio::sync::Mutex<Option<Running>>,
}

impl ImageWorker {
    pub fn new(name: &'static str) -> Self {
        Self { name, running: tokio::sync::Mutex::new(None) }
    }

    /// Loads the OCR models now, so a missing model shows at startup.
    pub async fn load(&self) -> Result<(), WorkerError> {
        let mut run = Duration::from_secs(60);
        self.call(LOAD, &[], None, &mut run, GRACE, 0)
            .await
            .map(|_| ())
    }

    /// `budget` counts only while the worker is this job's; what it uses is taken off.
    pub async fn ocr(
        &self,
        bytes: &[u8],
        budget: &mut Duration,
    ) -> Result<(String, bool), WorkerError> {
        let (out, partial) = self
            .call(OCR, bytes, None, budget, GRACE, 1 << 20)
            .await?;
        Ok((
            String::from_utf8_lossy(&out).into_owned(),
            partial,
        ))
    }

    pub async fn convert(
        &self,
        mime: &str,
        bytes: &[u8],
        max_out: usize,
    ) -> Result<Vec<u8>, WorkerError> {
        let kind = if mime == "image/gif" { CONVERT_GIF } else { CONVERT_BMP };
        let mut run = CONVERT_RUN;
        self.call(
            kind,
            bytes,
            Some(CONVERT_WAIT),
            &mut run,
            CONVERT_GRACE,
            max_out,
        )
        .await
        .map(|(out, _)| out)
    }

    async fn call(
        &self,
        kind: u8,
        payload: &[u8],
        wait: Option<Duration>,
        run: &mut Duration,
        grace: Duration,
        max_out: usize,
    ) -> Result<(Vec<u8>, bool), WorkerError> {
        let mut slot = match wait {
            Some(wait) => tokio::time::timeout(wait, self.running.lock())
                .await
                .map_err(|_| WorkerError::Late)?,
            None => self.running.lock().await,
        };
        let left = *run;
        if left.is_zero() {
            return Err(WorkerError::Late);
        }
        let started = Instant::now();
        let result = self
            .exchange(&mut slot, kind, payload, left, grace, max_out)
            .await;
        *run = run.saturating_sub(started.elapsed());
        result
    }

    async fn exchange(
        &self,
        slot: &mut Option<Running>,
        kind: u8,
        payload: &[u8],
        left: Duration,
        grace: Duration,
        max_out: usize,
    ) -> Result<(Vec<u8>, bool), WorkerError> {
        // An idle worker can die (the OOM killer may pick it); that says nothing about this image.
        let alive = slot
            .as_mut()
            .is_some_and(|running| matches!(running.child.try_wait(), Ok(None)));
        let (mut running, mut reused) = match slot.take() {
            Some(running) if alive => (running, true),
            _ => (
                self.spawn()
                    .map_err(|error| WorkerError::Crashed(format!("{error:#}")))?,
                false,
            ),
        };
        let mut head = Vec::with_capacity(13);
        head.push(kind);
        head.extend(
            (left
                .as_millis()
                .min(u128::from(u32::MAX)) as u32)
                .to_le_bytes(),
        );
        head.extend((max_out.min(u32::MAX as usize) as u32).to_le_bytes());
        head.extend((payload.len() as u32).to_le_bytes());
        let exchange = async {
            while let Err(error) = send(&mut running, &head, payload).await {
                if !std::mem::take(&mut reused) {
                    return Err(error);
                }
                running = self
                    .spawn()
                    .map_err(std::io::Error::other)?;
            }
            let mut reply = [0u8; 6];
            running
                .stdout
                .read_exact(&mut reply)
                .await?;
            if reply[0] > 1 {
                return Err(std::io::Error::other(format!(
                    "unknown status {}",
                    reply[0]
                )));
            }
            let len = u32::from_le_bytes([reply[2], reply[3], reply[4], reply[5]]) as usize;
            let limit = if reply[0] == 0 { max_out } else { 4096 };
            if len > limit {
                return Err(std::io::Error::other(format!(
                    "answer of {len} bytes"
                )));
            }
            let mut body = vec![0; len];
            running
                .stdout
                .read_exact(&mut body)
                .await?;
            Ok((reply[0], reply[1] != 0, body))
        };
        let result = tokio::time::timeout(left + grace, exchange).await;
        match result {
            Ok(Ok((0, partial, body))) => {
                *slot = Some(running);
                Ok((body, partial))
            }
            Ok(Ok((1, _, message))) => {
                *slot = Some(running);
                Err(WorkerError::Refused(
                    String::from_utf8_lossy(&message).into_owned(),
                ))
            }
            Ok(Ok((status, ..))) => Err(self.crashed(format!("unknown status {status}"))),
            Ok(Err(error)) => Err(self.crashed(error.to_string())),
            Err(_) => Err(self.crashed("no answer before the deadline".to_owned())),
        }
    }

    fn crashed(&self, reason: String) -> WorkerError {
        metrics::counter!("image_worker_crashes_total", "worker" => self.name).increment(1);
        tracing::warn!(worker = self.name, %reason, "image worker stopped; a new one starts on the next job");
        WorkerError::Crashed(reason)
    }

    fn spawn(&self) -> anyhow::Result<Running> {
        // Only scan settings, not the token or API keys.
        let env = std::env::vars().filter(|(key, _)| {
            // Other RTEN_* settings print to stdout, which carries replies.
            key.starts_with("SCAM_DETECT_")
                || matches!(
                    key.as_str(),
                    "RTEN_NUM_THREADS" | "RUST_LOG" | "TZ" | "NO_COLOR"
                )
        });
        let mut child = Command::new(std::env::current_exe()?)
            .arg(ARG)
            .env_clear()
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        metrics::counter!("image_worker_starts_total", "worker" => self.name).increment(1);
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("no worker stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no worker stdout"))?;
        Ok(Running { child, stdin, stdout })
    }
}

async fn send(running: &mut Running, head: &[u8], payload: &[u8]) -> std::io::Result<()> {
    running.stdin.write_all(head).await?;
    running.stdin.write_all(payload).await?;
    running.stdin.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: u8, max_out: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![kind];
        out.extend(60_000u32.to_le_bytes());
        out.extend(max_out.to_le_bytes());
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend(payload);
        out
    }

    fn answers(mut bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        while !bytes.is_empty() {
            let len = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
            out.push((bytes[0], bytes[6..6 + len].to_vec()));
            bytes = &bytes[6 + len..];
        }
        out
    }

    #[test]
    fn worker_answers_each_request_and_refuses_bad_ones() {
        let config = ScamConfig {
            enabled: true,
            queue_capacity: 1,
            max_images_per_message: 1,
            max_upload_bytes: 1 << 20,
            download_timeout: Duration::from_secs(1),
            scan_timeout: Duration::from_secs(30),
            job_ttl: Duration::from_secs(1),
            max_image_width: 1600,
            max_decoded_pixels: 16_777_216,
            worker_memory_mb: 256,
            text_limit: 3000,
            block_threshold: 0.8,
            review_threshold: 0.55,
            models_dir: "/nonexistent".into(),
        };
        let mut gif = Vec::new();
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(
                &mut std::io::Cursor::new(&mut gif),
                image::ImageFormat::Gif,
            )
            .unwrap();
        let input = [
            request(CONVERT_GIF, 1 << 20, &gif),
            request(CONVERT_GIF, 10, &gif),
            request(CONVERT_GIF, 1 << 20, b"junk"),
            request(99, 0, b""),
        ]
        .concat();
        let mut output = Vec::new();
        serve(&mut input.as_slice(), &mut output, &config).unwrap();
        let answers = answers(&output);
        assert_eq!(answers.len(), 4);
        assert_eq!(answers[0].0, 0);
        assert!(answers[0].1.starts_with(b"\x89PNG"));
        assert!(
            answers[1..]
                .iter()
                .all(|(status, _)| *status == 1),
            "{answers:?}"
        );
    }
}
