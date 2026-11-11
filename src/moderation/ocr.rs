use std::{path::Path, time::Instant};

use anyhow::Context as _;
use image::{DynamicImage, GenericImageView as _, ImageReader};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use rten_imageproc::{BoundingRect as _, RotatedRect};

use crate::config::ScamConfig;

pub const DETECTION_MODEL: &str = "text-detection.onnx";
pub const RECOGNITION_MODEL: &str = "text-recognition.onnx";
/// ocrs's `get_text` recognises every line at once, which used about 1 GB on dense text.
const LINES_PER_CALL: usize = 4;
/// Above this ocrs's line layout can blow up (a dot-grid PNG hit 740 MB), so we group rows here.
const MAX_LAYOUT_WORDS: usize = 2000;
/// Only for the fallback path; ocrs's layout is not capped, or dots could push scam text out.
const MAX_FALLBACK_LINES: usize = 200;
/// ocrs pads small images to its 800x600 input as floats, so each side counts as at least 800.
const DETECTION_INPUT_SIDE: u32 = 800;

pub struct Engine {
    inner: OcrEngine,
}

impl Engine {
    pub fn new(config: &ScamConfig) -> anyhow::Result<Self> {
        Self::load(&config.models_dir)
    }

    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let detection = Model::load_file(dir.join(DETECTION_MODEL))
            .with_context(|| format!("load {}", dir.join(DETECTION_MODEL).display()))?;
        let recognition = Model::load_file(dir.join(RECOGNITION_MODEL))
            .with_context(|| format!("load {}", dir.join(RECOGNITION_MODEL).display()))?;
        let inner = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection),
            recognition_model: Some(recognition),
            ..Default::default()
        })?;
        Ok(Self { inner })
    }

    /// Past `deadline` it stops between batches and returns what it has read, flagged partial.
    pub fn read(
        &self,
        image: &DynamicImage,
        config: &ScamConfig,
        deadline: Instant,
    ) -> anyhow::Result<(String, bool)> {
        let (width, height) = image.dimensions();
        // ocrs reads grey, RGB and RGBA as is; converting would hold a second copy.
        let converted;
        let bytes = match image {
            DynamicImage::ImageLuma8(buf) => buf.as_raw(),
            DynamicImage::ImageRgb8(buf) => buf.as_raw(),
            DynamicImage::ImageRgba8(buf) => buf.as_raw(),
            other => {
                converted = other.to_rgb8();
                converted.as_raw()
            }
        };
        let source = ImageSource::from_bytes(bytes, (width, height))?;
        let input = self.inner.prepare_input(source)?;
        let words = self.inner.detect_words(&input)?;
        let lines = if words.len() > MAX_LAYOUT_WORDS {
            let mut lines = rows(&words);
            lines.truncate(MAX_FALLBACK_LINES);
            lines
        } else {
            self.inner
                .find_text_lines(&input, &words)
        };
        let mut text = String::new();
        let mut chars = 0;
        let mut partial = false;
        for group in lines.chunks(LINES_PER_CALL) {
            if Instant::now() >= deadline {
                partial = true;
                break;
            }
            for line in self
                .inner
                .recognize_text(&input, group)?
                .into_iter()
                .flatten()
            {
                if !text.is_empty() {
                    text.push('\n');
                }
                let line = line.to_string();
                chars += line.chars().count() + 1;
                text.push_str(&line);
            }
            // The rest would be cut off below anyway.
            if chars >= config.text_limit {
                break;
            }
        }
        Ok((
            text.chars()
                .take(config.text_limit)
                .collect(),
            partial,
        ))
    }
}

/// Crude line grouping for when ocrs's layout wouldn't finish. Padding can force this path, so it
/// must still read scams.
fn rows(words: &[RotatedRect]) -> Vec<Vec<RotatedRect>> {
    let mut sorted = words.to_vec();
    sorted.sort_by(|a, b| a.center().y.total_cmp(&b.center().y));
    let mut rows: Vec<(f32, f32, Vec<RotatedRect>)> = Vec::new();
    for word in sorted {
        let rect = word.bounding_rect();
        let (top, bottom) = (rect.top(), rect.bottom());
        if let Some((row_top, row_bottom, row)) = rows.last_mut() {
            let overlap = bottom.min(*row_bottom) - top.max(*row_top);
            if overlap > 0.5 * (bottom - top).min(*row_bottom - *row_top) {
                *row_top = row_top.min(top);
                *row_bottom = row_bottom.max(bottom);
                row.push(word);
                continue;
            }
        }
        rows.push((top, bottom, vec![word]));
    }
    let mut lines = Vec::with_capacity(rows.len());
    for (_, _, mut row) in rows {
        row.sort_by(|a, b| a.center().x.total_cmp(&b.center().x));
        let mut line: Vec<RotatedRect> = Vec::new();
        for word in row {
            if let Some(last) = line.last() {
                let (prev, next) = (last.bounding_rect(), word.bounding_rect());
                let height = prev.height().max(next.height());
                if next.left() - prev.right() > 2.0 * height {
                    lines.push(std::mem::take(&mut line));
                }
            }
            line.push(word);
        }
        lines.push(line);
    }
    lines
}

/// One image, or two for a transparent one (see `grey`). Runs in the image worker.
pub fn decode(bytes: &[u8], config: &ScamConfig) -> anyhow::Result<Vec<DynamicImage>> {
    if !webp_frames_fit(bytes, config.max_decoded_pixels) {
        anyhow::bail!("WebP frame is larger than the pixel limit or nested");
    }
    if !gif_frames_fit(bytes, config.max_decoded_pixels) {
        anyhow::bail!("GIF frame data is out of spec or larger than its pixels need");
    }
    // A GIF frame can be far larger than its screen size, and a PNG decoder inflates its colour
    // profile on probe, so the decoder itself is capped.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(
        config
            .max_decoded_pixels
            .saturating_mul(4),
    );
    let reader = || -> anyhow::Result<ImageReader<std::io::Cursor<&[u8]>>> {
        let mut reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
        reader.limits(limits.clone());
        Ok(reader)
    };
    let (width, height) = reader()?.into_dimensions()?;
    // Resizing only shrinks the padded size, so check the original.
    let padded =
        u64::from(width.max(DETECTION_INPUT_SIDE)) * u64::from(height.max(DETECTION_INPUT_SIDE));
    if u64::from(width) * u64::from(height) > config.max_decoded_pixels
        || padded > config.max_decoded_pixels
    {
        anyhow::bail!("image has too many pixels ({width}x{height})");
    }
    Ok(grey(reader()?.decode()?)
        .into_iter()
        .map(|image| DynamicImage::ImageLuma8(resize(image, config.max_image_width)))
        .collect())
}

/// Discord's dark theme background (#313338) as grey.
const DARK: u32 = 0x33;

/// Rec. 601, which ocrs's models were trained with (`into_luma8` uses Rec. 709).
fn luma(r: u8, g: u8, b: u8) -> u32 {
    (299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000
}

/// ocrs ignores alpha, so transparent images are composited over both Discord themes; one alone
/// can be beaten by framing the image in the other's colour.
fn grey(image: DynamicImage) -> Vec<image::GrayImage> {
    let image = match image {
        DynamicImage::ImageLuma8(grey) => return vec![grey],
        image if !image.color().has_alpha() => {
            let rgb = image.into_rgb8();
            return vec![image::GrayImage::from_fn(rgb.width(), rgb.height(), |x, y| {
                let p = rgb.get_pixel(x, y);
                image::Luma([luma(p[0], p[1], p[2]) as u8])
            })];
        }
        image => image.into_rgba8(),
    };
    let opaque = image.pixels().all(|p| p[3] == u8::MAX);
    let backgrounds: &[u32] = if opaque { &[255] } else { &[255, DARK] };
    backgrounds
        .iter()
        .map(|&background| {
            image::GrayImage::from_fn(image.width(), image.height(), |x, y| {
                let p = image.get_pixel(x, y);
                let alpha = u32::from(p[3]);
                let shown = luma(p[0], p[1], p[2]) * alpha + background * (255 - alpha);
                image::Luma([(shown / 255) as u8])
            })
        })
        .collect()
}

/// image's WebP decoder sizes lossy frames from their own header, not the checked canvas. Nested
/// frames are refused.
fn webp_frames_fit(bytes: &[u8], max_pixels: u64) -> bool {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return true;
    }
    fn chunks_fit(mut data: &[u8], max_pixels: u64, in_frame: bool) -> bool {
        while data.len() >= 8 {
            let size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
            let end = data.len().min(8 + size);
            let payload = &data[8..end];
            let fits = match &data[..4] {
                b"VP8 " => {
                    payload.len() >= 10 && payload[3..6] == [0x9d, 0x01, 0x2a] && {
                        let width = u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff;
                        let height = u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff;
                        u64::from(width) * u64::from(height) <= max_pixels
                    }
                }
                b"VP8L" => {
                    payload.len() >= 5 && payload[0] == 0x2f && {
                        let bits =
                            u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
                        let width = u64::from(bits & 0x3fff) + 1;
                        let height = u64::from((bits >> 14) & 0x3fff) + 1;
                        width * height <= max_pixels
                    }
                }
                b"ANMF" => {
                    !in_frame && payload.len() >= 16 && chunks_fit(&payload[16..], max_pixels, true)
                }
                _ => true,
            };
            if !fits {
                return false;
            }
            if 8 + size >= data.len() {
                break;
            }
            data = data
                .get(8 + size + (size & 1)..)
                .unwrap_or_default();
        }
        true
    }
    chunks_fit(&bytes[12..], max_pixels, false)
}

/// LZW time follows the data, not the pixels (a GIF of clear codes draws nothing). A frame needs
/// at most 1.5 bytes per pixel, so the blocks are summed without decoding.
pub fn gif_frames_fit(bytes: &[u8], max_pixels: u64) -> bool {
    if !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) || bytes.len() < 13 {
        return true;
    }
    let table = |flags: u8| if flags & 0x80 == 0 { 0 } else { 3 << ((flags & 7) + 1) };
    // Data sub-blocks from `at`; the position after the terminator and their total length.
    let blocks = |mut at: usize| -> Option<(usize, u64)> {
        let mut total = 0u64;
        loop {
            let len = usize::from(*bytes.get(at)?);
            at += 1 + len;
            if len == 0 {
                return Some((at, total));
            }
            total += len as u64;
        }
    };
    let mut at = 13 + table(bytes[10]);
    loop {
        match bytes.get(at) {
            Some(0x21) => match blocks(at + 2) {
                Some((next, _)) => at = next,
                None => return true,
            },
            Some(0x2c) => {
                let Some(descriptor) = bytes.get(at + 1..at + 10) else {
                    return true;
                };
                let width = u64::from(u16::from_le_bytes([descriptor[4], descriptor[5]]));
                let height = u64::from(u16::from_le_bytes([descriptor[6], descriptor[7]]));
                at += 10 + table(descriptor[8]);
                match bytes.get(at) {
                    Some(&size) if size <= 8 => {}
                    Some(_) => return false,
                    None => return true,
                }
                let budget = (width * height).min(max_pixels) * 3 / 2 + 1024;
                // A truncated frame is checked as far as its bytes go.
                let mut data = 0u64;
                let mut next = at + 1;
                while let Some(&len) = bytes.get(next) {
                    next += 1 + usize::from(len);
                    data += u64::from(len);
                    if data > budget {
                        return false;
                    }
                    if len == 0 {
                        break;
                    }
                }
                at = next;
            }
            _ => return true,
        }
    }
}

/// Area average. `thumbnail_exact` lost blocky strokes; `resize_exact` holds a large float copy.
fn resize(image: image::GrayImage, max_width: u32) -> image::GrayImage {
    let (width, height) = image.dimensions();
    if width <= max_width {
        return image;
    }
    let new_height = (u64::from(height) * u64::from(max_width) / u64::from(width)).max(1) as u32;
    let cols = area_spans(width, max_width);
    let rows = area_spans(height, new_height);
    let source = image.as_raw();
    let mut row = vec![0f32; max_width as usize];
    let mut sum = vec![0f32; max_width as usize];
    let mut out = image::GrayImage::new(max_width, new_height);
    for (y, taps) in rows.iter().enumerate() {
        sum.fill(0.0);
        for &(sy, wy) in taps {
            let line = &source[sy as usize * width as usize..][..width as usize];
            for (value, taps) in row.iter_mut().zip(&cols) {
                *value = taps
                    .iter()
                    .map(|&(sx, wx)| f32::from(line[sx as usize]) * wx)
                    .sum();
            }
            for (total, value) in sum.iter_mut().zip(&row) {
                *total += value * wy;
            }
        }
        for (x, total) in sum.iter().enumerate() {
            out.put_pixel(
                x as u32,
                y as u32,
                image::Luma([total.round().clamp(0.0, 255.0) as u8]),
            );
        }
    }
    out
}

fn area_spans(from: u32, to: u32) -> Vec<Vec<(u32, f32)>> {
    let scale = f64::from(from) / f64::from(to);
    (0..to)
        .map(|o| {
            let (start, end) = (f64::from(o) * scale, f64::from(o + 1) * scale);
            (start.floor() as u32..(end.ceil() as u32).min(from))
                .filter_map(|i| {
                    let cover = end.min(f64::from(i) + 1.0) - start.max(f64::from(i));
                    (cover > 0.0).then_some((i, (cover / scale) as f32))
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::moderation::scoring::{Action, score_text};

    fn config() -> ScamConfig {
        ScamConfig {
            enabled: true,
            queue_capacity: 1,
            max_images_per_message: 3,
            max_upload_bytes: 10 << 20,
            download_timeout: Duration::from_secs(1),
            scan_timeout: Duration::from_secs(1),
            job_ttl: Duration::from_secs(1),
            max_image_width: 100,
            max_decoded_pixels: 1_000_000,
            worker_memory_mb: 256,
            text_limit: 3000,
            block_threshold: 0.8,
            review_threshold: 0.55,
            models_dir: std::env::var_os("OCRS_MODELS_DIR")
                .map(Into::into)
                .unwrap_or_default(),
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::new_rgb8(width, height)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Png,
            )
            .unwrap();
        out
    }

    #[test]
    fn fallback_rows_split_columns() {
        use rten_imageproc::{PointF, Vec2};
        let word = |x: f32, y: f32| {
            RotatedRect::new(
                PointF::from_yx(y, x),
                Vec2::from_yx(1., 0.),
                40.,
                10.,
            )
        };
        // 40x10 words: two side by side (10 px apart), a third far to the right (another
        // column), and one on the next line.
        let lines = rows(&[word(160., 51.), word(400., 50.), word(110., 50.), word(150., 80.)]);
        let xs: Vec<Vec<f32>> = lines
            .iter()
            .map(|line| {
                line.iter()
                    .map(|w| w.center().x)
                    .collect()
            })
            .collect();
        assert_eq!(xs, [vec![110., 160.], vec![400.], vec![150.]]);
    }

    #[test]
    fn rejects_images_over_pixel_limit_and_resizes_wide_ones() {
        assert!(decode(&png(2000, 1000), &config()).is_err());
        // 13,000 pixels, but padded to 800 wide for detection.
        assert!(decode(&png(10, 1300), &config()).is_err());
        let resized = decode(
            &png(100, 50),
            &ScamConfig { max_image_width: 50, ..config() },
        )
        .unwrap();
        assert_eq!(resized.len(), 1);
        assert_eq!(resized[0].dimensions(), (50, 25));
    }

    #[test]
    fn area_resize_averages_what_each_pixel_covers() {
        // Alternating one-pixel black and white columns average to grey, not to either.
        let stripes = image::GrayImage::from_fn(300, 30, |x, _| {
            image::Luma([if x % 2 == 0 { 0 } else { 255 }])
        });
        let out = resize(stripes, 150);
        assert_eq!(out.dimensions(), (150, 15));
        assert!(
            out.pixels()
                .all(|p| (120..=135).contains(&p.0[0])),
            "{:?}",
            out.get_pixel(0, 0)
        );
        let flat = resize(
            image::GrayImage::from_pixel(333, 7, image::Luma([200])),
            100,
        );
        assert!(flat.pixels().all(|p| p.0[0] == 200));
    }

    #[test]
    fn opaque_colour_uses_the_weights_ocrs_was_trained_with() {
        // Magenta on rgb(0,101,0) is equal under Rec. 709 and must not be under Rec. 601.
        let image = image::RgbImage::from_fn(2, 1, |x, _| {
            image::Rgb(if x == 0 { [255, 0, 255] } else { [0, 101, 0] })
        });
        let grey = grey(DynamicImage::ImageRgb8(image));
        assert_eq!(grey.len(), 1);
        assert!(grey[0][(0, 0)].0[0].abs_diff(grey[0][(1, 0)].0[0]) > 20);
    }

    #[test]
    fn transparent_images_are_read_over_both_theme_backgrounds() {
        // One flat colour everywhere; the "text" is the single opaque pixel.
        let painted = |rgb: [u8; 3]| {
            let mut image =
                image::RgbaImage::from_pixel(4, 1, image::Rgba([rgb[0], rgb[1], rgb[2], 0]));
            image.put_pixel(0, 0, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
            grey(DynamicImage::ImageRgba8(image))
        };
        let shown = |images: &[image::GrayImage]| -> Vec<(u8, u8)> {
            images
                .iter()
                .map(|g| (g[(0, 0)].0[0], g[(1, 0)].0[0]))
                .collect()
        };
        assert_eq!(
            shown(&painted([255, 255, 255])),
            [(255, 255), (255, 0x33)]
        );
        assert_eq!(
            shown(&painted([0x31, 0x33, 0x38])),
            [(50, 255), (50, 0x33)]
        );
        let opaque = grey(DynamicImage::ImageRgba8(
            image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255])),
        ));
        assert_eq!(opaque.len(), 1);
    }

    #[test]
    fn webp_frame_larger_than_its_canvas_is_refused() {
        let vp8 = |width: u16, height: u16| {
            let mut payload = vec![0x10, 0x02, 0x00, 0x9d, 0x01, 0x2a];
            payload.extend(width.to_le_bytes());
            payload.extend(height.to_le_bytes());
            let mut chunk = b"VP8 ".to_vec();
            chunk.extend((payload.len() as u32).to_le_bytes());
            chunk.extend(payload);
            chunk
        };
        let webp = |chunks: &[u8]| {
            let mut out = b"RIFF".to_vec();
            out.extend((chunks.len() as u32 + 4).to_le_bytes());
            out.extend(b"WEBP");
            out.extend(chunks);
            out
        };
        let vp8x = {
            let mut chunk = b"VP8X".to_vec();
            chunk.extend(10u32.to_le_bytes());
            chunk.extend([0, 0, 0, 0, 99, 0, 0, 99, 0, 0]);
            chunk
        };
        let huge = webp(&[vp8x.clone(), vp8(16383, 16383)].concat());
        assert!(!webp_frames_fit(&huge, 1_000_000));
        let error = decode(&huge, &config()).unwrap_err();
        assert!(error.to_string().contains("WebP"), "{error:?}");
        assert!(webp_frames_fit(
            &webp(&[vp8x.clone(), vp8(100, 100)].concat()),
            1_000_000
        ));
        let mut anmf = b"ANMF".to_vec();
        let frame = vp8(16383, 16383);
        anmf.extend((16 + frame.len() as u32).to_le_bytes());
        anmf.extend([0; 16]);
        anmf.extend(frame);
        assert!(!webp_frames_fit(&webp(&anmf), 1_000_000));
        assert!(webp_frames_fit(b"not a webp", 1));
        // Bytes after the last chunk read as a chunk running past the end; decoders ignore them.
        let small = [vp8x.clone(), vp8(100, 100)].concat();
        assert!(webp_frames_fit(
            &[webp(&small), b"XXXXXXXXXX".to_vec()].concat(),
            1_000_000
        ));
        let mut truncated = webp(&[vp8x.clone(), vp8(16383, 16383)].concat());
        truncated.truncate(truncated.len() - 2);
        truncated.extend([0x10, 0x02, 0x00, 0x9d, 0x01, 0x2a, 0xff, 0x3f, 0xff, 0x3f]);
        assert!(!webp_frames_fit(&truncated, 1_000_000));
        // A frame inside a frame is refused at the second level, however deep it goes.
        let mut nested = vp8(10, 10);
        for _ in 0..1_000 {
            let mut outer = b"ANMF".to_vec();
            outer.extend((16 + nested.len() as u32).to_le_bytes());
            outer.extend([0; 16]);
            outer.extend(nested);
            nested = outer;
        }
        assert!(!webp_frames_fit(&webp(&nested), 1_000_000));
    }

    #[test]
    fn gif_frames_must_fit_their_pixels() {
        let gif = |size: u8, blocks: usize| {
            let mut out = b"GIF89a\x0a\x00\x0a\x00\x00\x00\x00".to_vec();
            out.extend([0x21, 0xf9, 4, 0, 0, 0, 0, 0]);
            out.extend([0x2c, 0, 0, 0, 0, 10, 0, 10, 0, 0, size]);
            for _ in 0..blocks {
                out.push(255);
                out.extend([0u8; 255]);
            }
            out.extend([0, 0x3b]);
            out
        };
        assert!(gif_frames_fit(&gif(2, 2), 1_000_000));
        assert!(!gif_frames_fit(&gif(11, 1), 1_000_000));
        assert!(!gif_frames_fit(&gif(8, 10), 1_000_000));
        let mut truncated = gif(8, 10);
        truncated.truncate(400);
        assert!(gif_frames_fit(&truncated, 1_000_000));
        let mut real = Vec::new();
        let noise = image::RgbaImage::from_fn(300, 200, |x, y| {
            let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) as u8;
            image::Rgba([v, v.rotate_left(3), v.rotate_left(5), 255])
        });
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut real);
        for _ in 0..3 {
            encoder
                .encode_frame(image::Frame::new(noise.clone()))
                .unwrap();
        }
        drop(encoder);
        assert!(gif_frames_fit(&real, 1_000_000));
        assert!(gif_frames_fit(b"not a gif", 1));
    }

    #[test]
    fn gif_frame_larger_than_its_screen_is_refused() {
        let mut gif = Vec::new();
        DynamicImage::new_rgba8(1, 1)
            .write_to(
                &mut std::io::Cursor::new(&mut gif),
                image::ImageFormat::Gif,
            )
            .unwrap();
        let descriptor = gif
            .iter()
            .position(|&b| b == 0x2C)
            .unwrap();
        let huge = 11_000u16.to_le_bytes();
        gif[descriptor + 5..descriptor + 7].copy_from_slice(&huge);
        gif[descriptor + 7..descriptor + 9].copy_from_slice(&huge);
        let error = decode(&gif, &config()).unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<image::ImageError>(),
                Some(image::ImageError::Limits(_))
            ),
            "{error:?}"
        );
    }

    #[test]
    fn scam_samples_block_and_safe_samples_allow() {
        let (Some(models), Some(samples)) = (
            std::env::var_os("OCRS_MODELS_DIR"),
            std::env::var_os("SCAM_SAMPLES_DIR"),
        ) else {
            eprintln!(
                "skipping: set OCRS_MODELS_DIR and SCAM_SAMPLES_DIR to run the OCR sample check"
            );
            return;
        };
        let engine = Engine::load(Path::new(&models)).unwrap();
        let config =
            ScamConfig { max_image_width: 1600, max_decoded_pixels: 16_777_216, ..config() };
        for entry in std::fs::read_dir(samples).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let images = decode(&std::fs::read(&path).unwrap(), &config).unwrap();
            let deadline = Instant::now() + Duration::from_secs(60);
            let (action, text) = images
                .iter()
                .map(|image| {
                    let (text, _) = engine
                        .read(image, &config, deadline)
                        .unwrap();
                    let score = score_text(
                        &text,
                        config.block_threshold,
                        config.review_threshold,
                    );
                    (score.risk, score.action, text)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, action, text)| (action, text))
                .unwrap();
            let expected = if name.starts_with("safe") { Action::Allow } else { Action::Block };
            assert_eq!(action, expected, "{name}: {text}");
        }
    }
}
