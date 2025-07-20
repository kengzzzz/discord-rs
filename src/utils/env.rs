use std::{env, str::FromStr};

pub fn parse_env<T>(key: &str, default: &str) -> T
where
    T: FromStr + Default,
    <T as FromStr>::Err: std::fmt::Debug,
{
    env::var(key)
        .unwrap_or_else(|_| String::from(default))
        .parse()
        .unwrap_or_default()
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
