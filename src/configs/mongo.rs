use std::sync::LazyLock;

use mongodb::options::ClientOptions;

use crate::utils::env::{parse_env, parse_env_opt, secret_or_default};

pub struct MongoConfigs {
    pub auth_source: String,
    pub database: String,
    pub max_pool_size: u32,
    pub min_pool_size: u32,
    pub password: String,
    pub ssl: bool,
    pub uri: String,
    pub username: String,
    pub ca_file_path: Option<String>,
    pub cert_key_file_path: Option<String>,
    pub allow_invalid_certificates: Option<bool>,
}

impl MongoConfigs {
    pub fn apply_pool_size(&self, opts: &mut ClientOptions) {
        opts.max_pool_size = Some(self.max_pool_size);
        opts.min_pool_size = Some(self.min_pool_size);
    }
}

pub static MONGO_CONFIGS: LazyLock<MongoConfigs> = LazyLock::new(|| MongoConfigs {
    auth_source: parse_env("MONGO_AUTH_SOURCE", "admin"),
    database: parse_env("MONGO_DATABASE", "develop"),
    max_pool_size: parse_env("MONGO_MAX_POOL_SIZE", "30"),
    min_pool_size: parse_env("MONGO_MIN_POOL_SIZE", "10"),
    password: secret_or_default("MONGO_PASSWORD", "secret"),
    ssl: parse_env("MONGO_SSL", "false"),
    uri: parse_env(
        "MONGO_URI",
        "mongodb://mongo1:27017,mongo2:27017,mongo3:27017/?replicaSet=rs0",
    ),
    username: parse_env("MONGO_USERNAME", "homestead"),
    ca_file_path: parse_env_opt("MONGO_TLS_CA_FILE"),
    cert_key_file_path: parse_env_opt("MONGO_TLS_CERT_KEY_FILE"),
    allow_invalid_certificates: parse_env_opt("MONGO_TLS_INSECURE"),
});

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apply_pool_size_sets_configured_values_on_client_options() {
        let mut opts = ClientOptions::parse("mongodb://localhost:27017")
            .await
            .unwrap();
        let configs = MongoConfigs {
            auth_source: "admin".to_owned(),
            database: "test".to_owned(),
            max_pool_size: 42,
            min_pool_size: 7,
            password: "pw".to_owned(),
            ssl: false,
            uri: "mongodb://localhost:27017".to_owned(),
            username: "user".to_owned(),
            ca_file_path: None,
            cert_key_file_path: None,
            allow_invalid_certificates: None,
        };

        configs.apply_pool_size(&mut opts);

        assert_eq!(opts.max_pool_size, Some(42));
        assert_eq!(opts.min_pool_size, Some(7));
    }
}
