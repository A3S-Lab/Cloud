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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> ProviderAppConfig {
        ProviderAppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8090,
                body_limit_bytes: 1024,
            },
            provider: ProviderConfig {
                id: "local".to_string(),
                build: "test".to_string(),
                public_base_url: "http://127.0.0.1:8090".to_string(),
                state_path: PathBuf::from("state"),
                artifact_path: PathBuf::from("artifacts"),
                api_token: String::new(),
                max_input_bytes: 2048,
            },
        }
    }

    fn assert_invalid(config: &ProviderAppConfig, message: &str) {
        let error = config.validate().expect_err("provider config must fail");
        assert!(
            error.to_string().contains(message),
            "expected {error} to contain {message:?}"
        );
    }

    #[test]
    fn bundled_provider_config_loads_and_resolves_socket() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/runtime-provider.acl");
        let config = ProviderAppConfig::from_acl_file(path).expect("bundled provider config");

        assert_eq!(config.provider.id, "local");
        assert_eq!(
            config.socket_addr().expect("provider socket"),
            "127.0.0.1:8090".parse().expect("literal socket")
        );
    }

    #[test]
    fn missing_provider_config_file_reports_its_path() {
        let error = ProviderAppConfig::from_acl_file("missing-provider-config.acl")
            .expect_err("missing file must fail");
        assert!(error.to_string().contains("missing-provider-config.acl"));
    }

    #[test]
    fn provider_config_rejects_invalid_host_and_zero_limits() {
        let mut config = valid_config();
        config.server.host = "not-an-ip".to_string();
        assert_invalid(&config, "invalid server.host");

        let mut config = valid_config();
        config.server.port = 0;
        assert_invalid(&config, "server limits must be positive");

        let mut config = valid_config();
        config.server.body_limit_bytes = 0;
        assert_invalid(&config, "server limits must be positive");

        let mut config = valid_config();
        config.provider.max_input_bytes = 0;
        assert_invalid(&config, "max_input_bytes must be positive");
    }

    #[test]
    fn provider_config_rejects_invalid_identity_and_public_url() {
        let mut config = valid_config();
        config.provider.id = "invalid provider".to_string();
        assert_invalid(&config, "Runtime provider ID");

        let mut config = valid_config();
        config.provider.public_base_url = "not a URL".to_string();
        assert_invalid(&config, "invalid provider.public_base_url");
    }

    #[test]
    fn socket_addr_rechecks_the_host() {
        let mut config = valid_config();
        config.server.host = "invalid".to_string();

        assert!(config.socket_addr().is_err());
    }
}
