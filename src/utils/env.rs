use std::{env, fmt, fs, str::FromStr};

pub fn parse_env<T>(key: &str, default: &str) -> T
where
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Debug,
{
    match env::var(key) {
        Ok(raw) => raw.parse().unwrap_or_else(|error| {
            tracing::warn!(
                key,
                value = %raw,
                default,
                error = ?error,
                "invalid env var; using default"
            );
            parse_default_env(key, default)
        }),
        Err(_) => parse_default_env(key, default),
    }
}

fn parse_default_env<T>(key: &str, default: &str) -> T
where
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Debug,
{
    default.parse().unwrap_or_else(|error| {
        panic!("default value {default:?} for env var {key} failed to parse: {error:?}")
    })
}

pub fn parse_env_opt<T>(key: &str) -> Option<T>
where
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Debug,
{
    env::var(key).ok().and_then(|v| {
        if v.is_empty() {
            return None;
        }
        v.parse().ok()
    })
}

/// Deliberately implements neither `Debug` nor `Display`: the value can only
/// leave through [`Secret::into_value`], so it cannot be logged by accident.
pub enum Secret {
    /// From `<NAME>_FILE`, with one trailing line ending removed.
    File(String),
    /// Verbatim from the legacy `<NAME>` variable.
    Env(String),
}

impl Secret {
    pub fn into_value(self) -> String {
        match self {
            Secret::File(value) | Secret::Env(value) => value,
        }
    }
}

/// Every variant carries the variable and path only; file contents are never
/// stored, so neither `Debug` nor `Display` can leak the secret.
#[derive(Debug)]
pub enum SecretError {
    Unreadable { var: String, path: String, source: std::io::Error },
    NotUtf8 { var: String, path: String },
    Empty { var: String, path: String },
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretError::Unreadable { var, path, source } => {
                write!(
                    f,
                    "{var} points at {path}, which could not be read: {source}"
                )
            }
            SecretError::NotUtf8 { var, path } => {
                write!(
                    f,
                    "{var} points at {path}, which is not valid UTF-8"
                )
            }
            SecretError::Empty { var, path } => {
                write!(f, "{var} points at {path}, which is empty")
            }
        }
    }
}

impl std::error::Error for SecretError {}

/// Resolves a logical secret, preferring `<NAME>_FILE` over the legacy `<NAME>`.
///
/// A configured `<NAME>_FILE` that is unreadable, not UTF-8, or empty is an
/// error rather than a silent fallback to the legacy variable: a half-provisioned
/// secret should stop the process, not start it with the wrong credential.
pub fn secret(name: &str) -> Result<Option<Secret>, SecretError> {
    let file_var = format!("{name}_FILE");
    if let Ok(path) = env::var(&file_var)
        && !path.is_empty()
    {
        return read_secret_file(&file_var, &path).map(|value| Some(Secret::File(value)));
    }

    Ok(env::var(name).ok().map(Secret::Env))
}

/// Panics on a broken `<NAME>_FILE`, which surfaces as a startup abort via
/// [`crate::configs::init_secrets`].
pub fn secret_or_default(name: &str, default: &str) -> String {
    match secret(name) {
        Ok(Some(resolved)) => resolved.into_value(),
        Ok(None) => default.to_owned(),
        Err(error) => panic!("{error}"),
    }
}

fn read_secret_file(var: &str, path: &str) -> Result<String, SecretError> {
    let bytes = fs::read(path).map_err(|source| SecretError::Unreadable {
        var: var.to_owned(),
        path: path.to_owned(),
        source,
    })?;

    // `FromUtf8Error` keeps the offending bytes, so drop it rather than risk a
    // `Debug` of the error printing secret material.
    let raw = String::from_utf8(bytes)
        .map_err(|_| SecretError::NotUtf8 { var: var.to_owned(), path: path.to_owned() })?;

    let value = strip_one_line_ending(&raw);
    if value.is_empty() {
        return Err(SecretError::Empty { var: var.to_owned(), path: path.to_owned() });
    }

    Ok(value.to_owned())
}

/// Removes at most one trailing `\n` or `\r\n`, the line ending secret
/// provisioning tends to add. Anything else — further newlines, surrounding
/// whitespace — is treated as part of the credential and preserved.
fn strip_one_line_ending(raw: &str) -> &str {
    match raw.strip_suffix('\n') {
        Some(trimmed) => trimmed
            .strip_suffix('\r')
            .unwrap_or(trimmed),
        None => raw,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Serializes tests that mutate process-global env vars. Removes `keys`
    /// on acquisition and again on drop (including on panic).
    pub(crate) struct EnvGuard {
        keys: &'static [&'static str],
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        pub(crate) fn acquire(keys: &'static [&'static str]) -> Self {
            let lock = ENV_LOCK
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let guard = Self { keys, _lock: lock };
            guard.clear();
            guard
        }

        pub(crate) fn set(&self, key: &str, value: &str) {
            unsafe {
                std::env::set_var(key, value);
            }
        }

        fn clear(&self) {
            for key in self.keys {
                unsafe {
                    std::env::remove_var(key);
                }
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            self.clear();
        }
    }
}

#[cfg(test)]
#[path = "tests/env.rs"]
mod tests;
