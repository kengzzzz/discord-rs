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
#[path = "tests/env.rs"]
mod tests;
