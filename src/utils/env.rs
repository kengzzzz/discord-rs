use std::{env, str::FromStr};

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
