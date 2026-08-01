use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use a3s_boot::{parse_acl_config, BootError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub flow: FlowConfig,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub runtimes: BTreeMap<String, RuntimeProviderConfig>,
    pub gateway: GatewayConfig,
    pub memory: MemoryConfig,
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    #[serde(default = "default_body_limit")]
    pub body_limit_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub database_url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    pub audit_path: String,
    #[serde(default)]
    pub seed_sample: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlowConfig {
    #[serde(default = "default_queue_name")]
    pub queue_name: String,
    #[serde(default = "default_worker_poll_ms")]
    pub worker_poll_ms: u64,
    #[serde(default = "default_scheduler_poll_ms")]
    pub scheduler_poll_ms: u64,
    #[serde(default = "default_inflight_lease_seconds")]
    pub inflight_lease_seconds: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub default_provider: String,
    pub invocation_base_url: String,
    pub node_artifact_uri: String,
    pub node_artifact_digest: String,
    #[serde(default = "default_node_artifact_media_type")]
    pub node_artifact_media_type: String,
    #[serde(default)]
    pub node_command: Vec<String>,
    #[serde(default = "default_cpu_millis")]
    pub default_cpu_millis: u64,
    #[serde(default = "default_memory_bytes")]
    pub default_memory_bytes: u64,
    #[serde(default = "default_pids")]
    pub default_pids: u32,
    #[serde(default = "default_node_timeout_ms")]
    pub default_timeout_ms: u64,
    #[serde(default = "default_output_max_bytes")]
    pub output_max_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeProviderConfig {
    pub endpoint: String,
    #[serde(default)]
    pub api_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    pub base_url: String,
    #[serde(default)]
    pub api_key_reference: String,
    #[serde(default)]
    pub default_model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryConfig {
    pub base_url: String,
    #[serde(default)]
    pub api_key_reference: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub http_allowed_hosts: Vec<String>,
    #[serde(default = "default_max_http_response")]
    pub max_http_response_bytes: usize,
}

const fn default_body_limit() -> usize {
    2 * 1024 * 1024
}

const fn default_max_connections() -> usize {
    16
}

const fn default_worker_poll_ms() -> u64 {
    100
}

const fn default_scheduler_poll_ms() -> u64 {
    1_000
}

const fn default_inflight_lease_seconds() -> i64 {
    300
}

fn default_queue_name() -> String {
    "workflow".to_string()
}

fn default_node_artifact_media_type() -> String {
    "application/vnd.a3s.workflow.node-runner.v1".to_string()
}

const fn default_cpu_millis() -> u64 {
    500
}

const fn default_memory_bytes() -> u64 {
    256 * 1024 * 1024
}

const fn default_pids() -> u32 {
    128
}

const fn default_node_timeout_ms() -> u64 {
    120_000
}

const fn default_output_max_bytes() -> u64 {
    2 * 1024 * 1024
}

const fn default_max_http_response() -> usize {
    1024 * 1024
}

impl AppConfig {
    pub fn from_acl_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|error| {
            BootError::Internal(format!(
                "failed to read ACL configuration {}: {error}",
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
        for (label, value) in [
            ("server.port", u64::from(self.server.port)),
            (
                "server.body_limit_bytes",
                self.server.body_limit_bytes as u64,
            ),
            (
                "storage.max_connections",
                self.storage.max_connections as u64,
            ),
            ("flow.worker_poll_ms", self.flow.worker_poll_ms),
            ("flow.scheduler_poll_ms", self.flow.scheduler_poll_ms),
            (
                "runtime.default_cpu_millis",
                self.runtime.default_cpu_millis,
            ),
            (
                "runtime.default_memory_bytes",
                self.runtime.default_memory_bytes,
            ),
            ("runtime.default_pids", u64::from(self.runtime.default_pids)),
            (
                "runtime.default_timeout_ms",
                self.runtime.default_timeout_ms,
            ),
            ("runtime.output_max_bytes", self.runtime.output_max_bytes),
            (
                "security.max_http_response_bytes",
                self.security.max_http_response_bytes as u64,
            ),
        ] {
            if value == 0 {
                return Err(BootError::BadRequest(format!(
                    "{label} must be greater than zero"
                )));
            }
        }
        if self.flow.queue_name.trim().is_empty() || self.flow.inflight_lease_seconds <= 0 {
            return Err(BootError::BadRequest(
                "flow queue name and inflight lease must be positive".to_string(),
            ));
        }
        let database_url = parse_url("storage.database_url", &self.storage.database_url)?;
        if !matches!(database_url.scheme(), "postgres" | "postgresql") {
            return Err(BootError::BadRequest(
                "storage.database_url must use postgres:// or postgresql://".to_string(),
            ));
        }
        for (label, value) in [
            ("gateway.base_url", self.gateway.base_url.as_str()),
            ("memory.base_url", self.memory.base_url.as_str()),
            (
                "runtime.invocation_base_url",
                self.runtime.invocation_base_url.as_str(),
            ),
            (
                "runtime.node_artifact_uri",
                self.runtime.node_artifact_uri.as_str(),
            ),
        ] {
            parse_url(label, value)?;
        }
        validate_digest(&self.runtime.node_artifact_digest)?;
        if self.runtimes.is_empty() || !self.runtimes.contains_key(&self.runtime.default_provider) {
            return Err(BootError::BadRequest(format!(
                "runtime.default_provider {:?} is not configured",
                self.runtime.default_provider
            )));
        }
        for (provider, config) in &self.runtimes {
            a3s_runtime::ProviderId::parse(provider.clone())
                .map_err(|error| BootError::BadRequest(error.to_string()))?;
            parse_url(&format!("runtimes.{provider}.endpoint"), &config.endpoint)?;
        }
        Ok(())
    }

    pub fn socket_addr(&self) -> Result<SocketAddr> {
        let host = self
            .server
            .host
            .parse::<IpAddr>()
            .map_err(|error| BootError::BadRequest(format!("invalid server.host: {error}")))?;
        Ok(SocketAddr::new(host, self.server.port))
    }
}

fn parse_url(label: &str, value: &str) -> Result<url::Url> {
    url::Url::parse(value)
        .map_err(|error| BootError::BadRequest(format!("invalid {label}: {error}")))
}

fn validate_digest(value: &str) -> Result<()> {
    let valid = value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if valid {
        Ok(())
    } else {
        Err(BootError::BadRequest(
            "runtime.node_artifact_digest must be a sha256 digest".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    type ConfigMutation = (&'static str, fn(&mut AppConfig));

    fn valid_config() -> AppConfig {
        AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                cors_origins: Vec::new(),
                body_limit_bytes: 1024,
            },
            storage: StorageConfig {
                database_url: "postgres://workflow:workflow@127.0.0.1/workflow".to_string(),
                max_connections: 4,
                audit_path: ".a3s/test-audit.jsonl".to_string(),
                seed_sample: false,
            },
            flow: FlowConfig {
                queue_name: "workflow-test".to_string(),
                worker_poll_ms: 10,
                scheduler_poll_ms: 20,
                inflight_lease_seconds: 30,
            },
            runtime: RuntimeConfig {
                default_provider: "local".to_string(),
                invocation_base_url: "http://127.0.0.1:8080".to_string(),
                node_artifact_uri: "file:///usr/local/bin/a3s-workflow-node".to_string(),
                node_artifact_digest: format!("sha256:{}", "a".repeat(64)),
                node_artifact_media_type: "application/vnd.a3s.workflow.node-runner.v1".to_string(),
                node_command: Vec::new(),
                default_cpu_millis: 500,
                default_memory_bytes: 256 * 1024 * 1024,
                default_pids: 128,
                default_timeout_ms: 120_000,
                output_max_bytes: 2 * 1024 * 1024,
            },
            runtimes: BTreeMap::from([(
                "local".to_string(),
                RuntimeProviderConfig {
                    endpoint: "http://127.0.0.1:8090".to_string(),
                    api_token: String::new(),
                },
            )]),
            gateway: GatewayConfig {
                base_url: "http://127.0.0.1:9877/v1".to_string(),
                api_key_reference: String::new(),
                default_model: "test-model".to_string(),
            },
            memory: MemoryConfig {
                base_url: "http://127.0.0.1:8080/api/v1".to_string(),
                api_key_reference: String::new(),
            },
            security: SecurityConfig {
                http_allowed_hosts: vec!["api.example.test".to_string()],
                max_http_response_bytes: 1024,
            },
        }
    }

    fn assert_invalid(config: &AppConfig, message: &str) {
        let error = config.validate().expect_err("application config must fail");
        assert!(
            error.to_string().contains(message),
            "expected {error} to contain {message:?}"
        );
    }

    #[test]
    fn bundled_application_config_loads_and_resolves_socket() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/workflow.acl");
        let config = AppConfig::from_acl_file(path).expect("bundled workflow config");

        assert_eq!(config.runtime.default_provider, "local");
        assert_eq!(
            config.socket_addr().expect("application socket"),
            "127.0.0.1:8080".parse().expect("literal socket")
        );
    }

    #[test]
    fn missing_application_config_file_reports_its_path() {
        let error = AppConfig::from_acl_file("missing-workflow-config.acl")
            .expect_err("missing file must fail");
        assert!(error.to_string().contains("missing-workflow-config.acl"));
    }

    #[test]
    fn application_config_rejects_invalid_host_and_every_zero_limit() {
        let mut config = valid_config();
        config.server.host = "localhost".to_string();
        assert_invalid(&config, "invalid server.host");

        let cases: [ConfigMutation; 10] = [
            ("server.port", |value| value.server.port = 0),
            ("server.body_limit_bytes", |value| {
                value.server.body_limit_bytes = 0
            }),
            ("storage.max_connections", |value| {
                value.storage.max_connections = 0
            }),
            ("flow.worker_poll_ms", |value| value.flow.worker_poll_ms = 0),
            ("flow.scheduler_poll_ms", |value| {
                value.flow.scheduler_poll_ms = 0
            }),
            ("runtime.default_cpu_millis", |value| {
                value.runtime.default_cpu_millis = 0
            }),
            ("runtime.default_memory_bytes", |value| {
                value.runtime.default_memory_bytes = 0
            }),
            ("runtime.default_pids", |value| {
                value.runtime.default_pids = 0
            }),
            ("runtime.default_timeout_ms", |value| {
                value.runtime.default_timeout_ms = 0
            }),
            ("runtime.output_max_bytes", |value| {
                value.runtime.output_max_bytes = 0
            }),
        ];
        for (label, mutate) in cases {
            let mut config = valid_config();
            mutate(&mut config);
            assert_invalid(&config, label);
        }

        let mut config = valid_config();
        config.security.max_http_response_bytes = 0;
        assert_invalid(&config, "security.max_http_response_bytes");
    }

    #[test]
    fn application_config_requires_a_named_positive_flow_queue() {
        let mut config = valid_config();
        config.flow.queue_name = "  ".to_string();
        assert_invalid(&config, "flow queue name");

        let mut config = valid_config();
        config.flow.inflight_lease_seconds = 0;
        assert_invalid(&config, "flow queue name");
    }

    #[test]
    fn application_config_accepts_only_postgresql_as_source_of_truth() {
        for database_url in ["sqlite://workflow.db", "redis://127.0.0.1/workflow"] {
            let mut config = valid_config();
            config.storage.database_url = database_url.to_string();
            assert_invalid(&config, "must use postgres:// or postgresql://");
        }

        let mut config = valid_config();
        config.storage.database_url = "not a URL".to_string();
        assert_invalid(&config, "invalid storage.database_url");
    }

    #[test]
    fn application_config_rejects_invalid_service_urls_and_artifact_digest() {
        let cases: [ConfigMutation; 4] = [
            ("gateway.base_url", |value| {
                value.gateway.base_url = "not a URL".to_string()
            }),
            ("memory.base_url", |value| {
                value.memory.base_url = "not a URL".to_string()
            }),
            ("runtime.invocation_base_url", |value| {
                value.runtime.invocation_base_url = "not a URL".to_string()
            }),
            ("runtime.node_artifact_uri", |value| {
                value.runtime.node_artifact_uri = "not a URL".to_string()
            }),
        ];
        for (label, mutate) in cases {
            let mut config = valid_config();
            mutate(&mut config);
            assert_invalid(&config, label);
        }

        for digest in ["sha256:abc", &format!("md5:{}", "a".repeat(64))] {
            let mut config = valid_config();
            config.runtime.node_artifact_digest = digest.to_string();
            assert_invalid(&config, "must be a sha256 digest");
        }
    }

    #[test]
    fn application_config_requires_valid_configured_runtime_provider() {
        let mut config = valid_config();
        config.runtime.default_provider = "missing".to_string();
        assert_invalid(&config, "is not configured");

        let mut config = valid_config();
        let provider = config.runtimes.remove("local").expect("local runtime");
        config
            .runtimes
            .insert("invalid provider".to_string(), provider);
        config.runtime.default_provider = "invalid provider".to_string();
        assert_invalid(&config, "Runtime provider ID");

        let mut config = valid_config();
        config
            .runtimes
            .get_mut("local")
            .expect("local runtime")
            .endpoint = "not a URL".to_string();
        assert_invalid(&config, "runtimes.local.endpoint");
    }

    #[test]
    fn socket_addr_rechecks_the_host() {
        let mut config = valid_config();
        config.server.host = "invalid".to_string();

        assert!(config.socket_addr().is_err());
    }
}
