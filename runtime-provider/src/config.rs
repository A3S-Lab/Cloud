use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use a3s_boot::{parse_acl_config, BootError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderAppConfig {
    pub server: ServerConfig,
    pub provider: ProviderConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_body_limit")]
    pub body_limit_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    #[serde(default = "default_build")]
    pub build: String,
    pub public_base_url: String,
    pub state_path: PathBuf,
    pub artifact_path: PathBuf,
    #[serde(default)]
    pub api_token: String,
    #[serde(default = "default_input_limit")]
    pub max_input_bytes: u64,
}

const fn default_body_limit() -> usize {
    2 * 1024 * 1024
}

const fn default_input_limit() -> u64 {
    8 * 1024 * 1024
}

fn default_build() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

impl ProviderAppConfig {
    pub fn from_acl_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|error| {
            BootError::Internal(format!(
                "failed to read provider ACL {}: {error}",
                path.display()
            ))
        })?;
        let config: Self = parse_acl_config(&source)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.server
            .host
            .parse::<IpAddr>()
            .map_err(|error| BootError::BadRequest(format!("invalid server.host: {error}")))?;
        if self.server.port == 0 || self.server.body_limit_bytes == 0 {
            return Err(BootError::BadRequest(
                "provider server limits must be positive".to_string(),
            ));
        }
        if self.provider.max_input_bytes == 0 {
            return Err(BootError::BadRequest(
                "provider.max_input_bytes must be positive".to_string(),
            ));
        }
        a3s_runtime::ProviderId::parse(self.provider.id.clone())
            .map_err(|error| BootError::BadRequest(error.to_string()))?;
        url::Url::parse(&self.provider.public_base_url).map_err(|error| {
            BootError::BadRequest(format!("invalid provider.public_base_url: {error}"))
        })?;
        Ok(())
    }

    pub fn socket_addr(&self) -> Result<SocketAddr> {
        Ok(SocketAddr::new(
            self.server
                .host
                .parse::<IpAddr>()
                .map_err(|error| BootError::BadRequest(error.to_string()))?,
            self.server.port,
        ))
    }
}
