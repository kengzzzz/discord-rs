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

const SECRET_KEYS: &[&str] = &["TEST_SECRET", "TEST_SECRET_FILE"];
const SECRET_VALUE: &str = "s3cr3t-token-value";

struct TempSecret {
    path: std::path::PathBuf,
}

impl TempSecret {
    fn new(label: &str, contents: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "discord-bot-secret-test-{}-{label}",
            std::process::id()
        ));
        std::fs::write(&path, contents).expect("write temp secret");
        Self { path }
    }

    fn path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempSecret {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn secret_value(key: &str) -> Option<String> {
    secret(key)
        .expect("secret resolved")
        .map(Secret::into_value)
}

/// `expect_err` would require `Secret: Debug`, which it deliberately is not.
fn secret_error(key: &str) -> SecretError {
    match secret(key) {
        Ok(_) => panic!("expected {key} to fail"),
        Err(error) => error,
    }
}

#[test]
fn secret_prefers_file_over_legacy_env() {
    let file = TempSecret::new("precedence", b"from-file\n");
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET", "from-env");
    env.set("TEST_SECRET_FILE", &file.path());

    assert!(matches!(
        secret("TEST_SECRET"),
        Ok(Some(Secret::File(ref value))) if value == "from-file"
    ));
}

#[test]
fn secret_falls_back_to_legacy_env() {
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET", SECRET_VALUE);

    assert!(matches!(
        secret("TEST_SECRET"),
        Ok(Some(Secret::Env(ref value))) if value == SECRET_VALUE
    ));
}

#[test]
fn secret_falls_back_when_file_variable_is_empty() {
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET", SECRET_VALUE);
    env.set("TEST_SECRET_FILE", "");

    assert_eq!(
        secret_value("TEST_SECRET").as_deref(),
        Some(SECRET_VALUE)
    );
}

#[test]
fn secret_is_absent_when_neither_variable_is_set() {
    let _env = EnvGuard::acquire(SECRET_KEYS);

    assert!(secret_value("TEST_SECRET").is_none());
}

#[test]
fn secret_strips_exactly_one_trailing_newline() {
    let file = TempSecret::new("lf", b"line-fed-secret\n");
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET_FILE", &file.path());

    assert_eq!(
        secret_value("TEST_SECRET").as_deref(),
        Some("line-fed-secret")
    );
}

#[test]
fn secret_strips_exactly_one_trailing_crlf() {
    let file = TempSecret::new("crlf", b"windows-secret\r\n");
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET_FILE", &file.path());

    assert_eq!(
        secret_value("TEST_SECRET").as_deref(),
        Some("windows-secret")
    );
}

#[test]
fn secret_keeps_additional_newlines_and_surrounding_spaces() {
    let file = TempSecret::new("preserve", b"  pad ded \n\n");
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET_FILE", &file.path());

    assert_eq!(
        secret_value("TEST_SECRET").as_deref(),
        Some("  pad ded \n")
    );
}

#[test]
fn secret_keeps_value_without_trailing_newline() {
    let file = TempSecret::new("bare", b"no-newline");
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET_FILE", &file.path());

    assert_eq!(
        secret_value("TEST_SECRET").as_deref(),
        Some("no-newline")
    );
}

#[test]
fn secret_fails_when_configured_file_is_missing() {
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET", SECRET_VALUE);
    env.set(
        "TEST_SECRET_FILE",
        "/nonexistent/discord-bot/secret",
    );

    let error = secret_error("TEST_SECRET");

    assert!(matches!(error, SecretError::Unreadable { .. }));
    assert!(!format!("{error}").contains(SECRET_VALUE));
}

#[test]
fn secret_fails_when_configured_file_is_empty() {
    let file = TempSecret::new("empty", b"");
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET_FILE", &file.path());

    let error = secret_error("TEST_SECRET");

    assert!(matches!(error, SecretError::Empty { .. }));
}

#[test]
fn secret_fails_when_configured_file_holds_only_a_newline() {
    let file = TempSecret::new("newline-only", b"\n");
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET_FILE", &file.path());

    let error = secret_error("TEST_SECRET");

    assert!(matches!(error, SecretError::Empty { .. }));
}

#[test]
fn secret_errors_name_the_variable_and_path_but_never_the_content() {
    let mut contents = SECRET_VALUE.as_bytes().to_vec();
    contents.push(0xff);
    let file = TempSecret::new("not-utf8", &contents);
    let path = file.path();
    let env = EnvGuard::acquire(SECRET_KEYS);
    env.set("TEST_SECRET_FILE", &path);

    let error = secret_error("TEST_SECRET");

    assert!(matches!(error, SecretError::NotUtf8 { .. }));
    let rendered = format!("{error}");
    let debugged = format!("{error:?}");
    assert!(
        rendered.contains("TEST_SECRET_FILE"),
        "{rendered}"
    );
    assert!(rendered.contains(&path), "{rendered}");
    assert!(
        !rendered.contains(SECRET_VALUE),
        "Display leaked the secret"
    );
    assert!(
        !debugged.contains(SECRET_VALUE),
        "Debug leaked the secret"
    );
}

#[test]
fn secret_or_default_uses_default_only_when_unset() {
    let file = TempSecret::new("or-default", b"file-wins\n");
    let env = EnvGuard::acquire(SECRET_KEYS);

    assert_eq!(
        secret_or_default("TEST_SECRET", "fallback"),
        "fallback"
    );

    env.set("TEST_SECRET", "env-wins");
    assert_eq!(
        secret_or_default("TEST_SECRET", "fallback"),
        "env-wins"
    );

    env.set("TEST_SECRET_FILE", &file.path());
    assert_eq!(
        secret_or_default("TEST_SECRET", "fallback"),
        "file-wins"
    );
}
