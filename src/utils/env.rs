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
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_parse_env_returns_value() {
        let key = "TEST_PARSE_ENV_VALUE";
        unsafe {
            env::set_var(key, "42");
        }
        assert_eq!(parse_env::<u32>(key, "0"), 42);
        unsafe {
            env::remove_var(key);
        }
    }

    #[test]
    fn test_parse_env_returns_default_when_missing() {
        let key = "TEST_PARSE_ENV_MISSING";
        unsafe {
            env::remove_var(key);
        }
        assert_eq!(parse_env::<u32>(key, "7"), 7);
    }

    #[test]
    fn test_parse_env_returns_type_default_on_invalid() {
        let key = "TEST_PARSE_ENV_INVALID";
        unsafe {
            env::set_var(key, "invalid");
        }
        assert_eq!(parse_env::<u32>(key, "5"), 0);
        unsafe {
            env::remove_var(key);
        }
    }

    #[test]
    fn test_parse_env_opt_some() {
        let key = "TEST_PARSE_ENV_OPT_SOME";
        unsafe {
            env::set_var(key, "13");
        }
        assert_eq!(parse_env_opt::<u32>(key), Some(13));
        unsafe {
            env::remove_var(key);
        }
    }

    #[test]
    fn test_parse_env_opt_none_when_unset() {
        let key = "TEST_PARSE_ENV_OPT_UNSET";
        unsafe {
            env::remove_var(key);
        }
        assert_eq!(parse_env_opt::<u32>(key), None);
    }

    #[test]
    fn test_parse_env_opt_none_when_empty() {
        let key = "TEST_PARSE_ENV_OPT_EMPTY";
        unsafe {
            env::set_var(key, "");
        }
        assert_eq!(parse_env_opt::<u32>(key), None);
        unsafe {
            env::remove_var(key);
        }
    }
}
