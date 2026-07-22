use super::test_support::EnvGuard;
use super::*;

#[test]
fn test_parse_env_returns_value() {
    let key = "TEST_PARSE_ENV_VALUE";
    let env = EnvGuard::acquire(&["TEST_PARSE_ENV_VALUE"]);
    env.set(key, "42");
    assert_eq!(parse_env::<u32>(key, "0"), 42);
}

#[test]
fn test_parse_env_returns_default_when_missing() {
    let key = "TEST_PARSE_ENV_MISSING";
    let _env = EnvGuard::acquire(&["TEST_PARSE_ENV_MISSING"]);
    assert_eq!(parse_env::<u32>(key, "7"), 7);
}

#[test]
fn test_parse_env_returns_configured_default_on_invalid() {
    let key = "TEST_PARSE_ENV_INVALID";
    let env = EnvGuard::acquire(&["TEST_PARSE_ENV_INVALID"]);
    env.set(key, "invalid");
    assert_eq!(parse_env::<u32>(key, "5"), 5);
}

#[test]
fn test_parse_env_opt_some() {
    let key = "TEST_PARSE_ENV_OPT_SOME";
    let env = EnvGuard::acquire(&["TEST_PARSE_ENV_OPT_SOME"]);
    env.set(key, "13");
    assert_eq!(parse_env_opt::<u32>(key), Some(13));
}

#[test]
fn test_parse_env_opt_none_when_unset() {
    let key = "TEST_PARSE_ENV_OPT_UNSET";
    let _env = EnvGuard::acquire(&["TEST_PARSE_ENV_OPT_UNSET"]);
    assert_eq!(parse_env_opt::<u32>(key), None);
}

#[test]
fn test_parse_env_opt_none_when_empty() {
    let key = "TEST_PARSE_ENV_OPT_EMPTY";
    let env = EnvGuard::acquire(&["TEST_PARSE_ENV_OPT_EMPTY"]);
    env.set(key, "");
    assert_eq!(parse_env_opt::<u32>(key), None);
}
