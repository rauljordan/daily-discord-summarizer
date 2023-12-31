use config::{Config, ConfigError};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub service: ServiceConfig,
    #[allow(unused)]
    pub discord: DiscordConfig,
}

#[derive(Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Deserialize)]
pub struct ServiceConfig {
    pub produce_digest_interval_seconds: u64,
    pub message_log_directory: PathBuf,
    pub port: u16,
    pub host: String,
    pub max_gpt_request_tokens: usize,
}

#[derive(Deserialize)]
pub struct DiscordConfig {
    #[allow(unused)]
    pub channel_ids: Vec<String>,
}

impl AppConfig {
    pub fn load_from_file(file_path: &str) -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(config::File::with_name(file_path))
            .build()?;

        config.try_deserialize::<Self>()
    }
}
