use std::sync::LazyLock;

use crate::utils::env::{parse_env, parse_env_opt};

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

pub static MONGO_CONFIGS: LazyLock<MongoConfigs> = LazyLock::new(|| MongoConfigs {
    auth_source: parse_env("MONGO_AUTH_SOURCE", "admin"),
    database: parse_env("MONGO_DATABASE", "develop"),
    max_pool_size: parse_env("MONGO_MAX_POOL_SIZE", "30"),
    min_pool_size: parse_env("MONGO_MIN_POOL_SIZE", "10"),
    password: parse_env("MONGO_PASSWORD", "secret"),
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
