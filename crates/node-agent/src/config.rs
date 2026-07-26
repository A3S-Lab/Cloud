use a3s_acl::{Block, Document, Value};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneConfig {
    pub enrollment_url: Url,
    pub node_control_url: Url,
    pub enrollment_token_env: String,
    pub server_ca_file: PathBuf,
    pub max_response_bytes: usize,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub artifact_transfer_timeout_ms: u64,
    pub long_poll_margin_ms: u64,
    pub retry_initial_ms: u64,
    pub retry_max_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactConfig {
    pub max_blob_bytes: u64,
    pub max_entries: usize,
    pub max_file_bytes: u64,
    pub max_expanded_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeConfig {
    pub name: String,
    pub state_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogShippingConfig {
    pub poll_interval_ms: u64,
    pub max_batch_chunks: u16,
    pub max_batch_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerConfig {
    pub socket: String,
    pub namespace: String,
    pub operation_timeout_ms: u64,
    pub secret_memory_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayControlConfig {
    pub management_url: Url,
    pub auth_token_env: String,
    pub certificate_directory: PathBuf,
    pub connect_timeout_ms: u64,
    pub apply_timeout_ms: u64,
    pub readiness_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAgentConfig {
    pub control_plane: ControlPlaneConfig,
    pub artifacts: ArtifactConfig,
    pub node: NodeConfig,
    pub logs: LogShippingConfig,
    pub docker: DockerConfig,
    pub gateway: GatewayControlConfig,
}

impl NodeAgentConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        let document = a3s_acl::parse(source)
            .map_err(|error| ConfigError::Invalid(format!("invalid A3S ACL: {error}")))?;
        validate_root(&document)?;
        let control_plane = one_block(&document, "control_plane")?;
        validate_block(
            control_plane,
            &[
                "enrollment_url",
                "node_control_url",
                "enrollment_token_env",
                "server_ca_file",
                "max_response_bytes",
                "connect_timeout_ms",
                "request_timeout_ms",
                "artifact_transfer_timeout_ms",
                "long_poll_margin_ms",
                "retry_initial_ms",
                "retry_max_ms",
            ],
        )?;
        let artifacts = one_block(&document, "artifacts")?;
        validate_block(
            artifacts,
            &[
                "max_blob_bytes",
                "max_entries",
                "max_file_bytes",
                "max_expanded_bytes",
            ],
        )?;
        let node = one_block(&document, "node")?;
        validate_block(node, &["name", "state_dir"])?;
        let logs = one_block(&document, "logs")?;
        validate_block(
            logs,
            &["poll_interval_ms", "max_batch_chunks", "max_batch_bytes"],
        )?;
        let docker = one_block(&document, "docker")?;
        validate_block(
            docker,
            &[
                "socket",
                "namespace",
                "operation_timeout_ms",
                "secret_memory_dir",
            ],
        )?;
        let gateway = one_block(&document, "gateway")?;
        validate_block(
            gateway,
            &[
                "management_url",
                "auth_token_env",
                "certificate_directory",
                "connect_timeout_ms",
                "apply_timeout_ms",
                "readiness_timeout_ms",
            ],
        )?;

        let config = Self {
            control_plane: ControlPlaneConfig {
                enrollment_url: endpoint(
                    "control_plane.enrollment_url",
                    &string(control_plane, "enrollment_url")?,
                    true,
                    false,
                )?,
                node_control_url: endpoint(
                    "control_plane.node_control_url",
                    &string(control_plane, "node_control_url")?,
                    false,
                    true,
                )?,
                enrollment_token_env: string(control_plane, "enrollment_token_env")?,
                server_ca_file: PathBuf::from(string(control_plane, "server_ca_file")?),
                max_response_bytes: integer(control_plane, "max_response_bytes")?,
                connect_timeout_ms: integer(control_plane, "connect_timeout_ms")?,
                request_timeout_ms: integer(control_plane, "request_timeout_ms")?,
                artifact_transfer_timeout_ms: integer(
                    control_plane,
                    "artifact_transfer_timeout_ms",
                )?,
                long_poll_margin_ms: integer(control_plane, "long_poll_margin_ms")?,
                retry_initial_ms: integer(control_plane, "retry_initial_ms")?,
                retry_max_ms: integer(control_plane, "retry_max_ms")?,
            },
            artifacts: ArtifactConfig {
                max_blob_bytes: integer(artifacts, "max_blob_bytes")?,
                max_entries: integer(artifacts, "max_entries")?,
                max_file_bytes: integer(artifacts, "max_file_bytes")?,
                max_expanded_bytes: integer(artifacts, "max_expanded_bytes")?,
            },
            node: NodeConfig {
                name: string(node, "name")?,
                state_dir: PathBuf::from(string(node, "state_dir")?),
            },
            logs: LogShippingConfig {
                poll_interval_ms: integer(logs, "poll_interval_ms")?,
                max_batch_chunks: integer(logs, "max_batch_chunks")?,
                max_batch_bytes: integer(logs, "max_batch_bytes")?,
            },
            docker: DockerConfig {
                socket: string(docker, "socket")?,
                namespace: string(docker, "namespace")?,
                operation_timeout_ms: integer(docker, "operation_timeout_ms")?,
                secret_memory_dir: PathBuf::from(string(docker, "secret_memory_dir")?),
            },
            gateway: GatewayControlConfig {
                management_url: endpoint(
                    "gateway.management_url",
                    &string(gateway, "management_url")?,
                    true,
                    false,
                )?,
                auth_token_env: string(gateway, "auth_token_env")?,
                certificate_directory: PathBuf::from(string(gateway, "certificate_directory")?),
                connect_timeout_ms: integer(gateway, "connect_timeout_ms")?,
                apply_timeout_ms: integer(gateway, "apply_timeout_ms")?,
                readiness_timeout_ms: integer(gateway, "readiness_timeout_ms")?,
            },
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !valid_env_name(&self.control_plane.enrollment_token_env) {
            return Err(ConfigError::Invalid(
                "control_plane.enrollment_token_env must be an uppercase environment variable name"
                    .into(),
            ));
        }
        validate_path(
            "control_plane.server_ca_file",
            &self.control_plane.server_ca_file,
        )?;
        validate_path("node.state_dir", &self.node.state_dir)?;
        validate_path(
            "gateway.certificate_directory",
            &self.gateway.certificate_directory,
        )?;
        if !self.gateway.certificate_directory.is_absolute()
            || self
                .gateway
                .certificate_directory
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(ConfigError::Invalid(
                "gateway.certificate_directory must be an absolute normalized directory".into(),
            ));
        }
        if self.node.name.trim().is_empty()
            || self.node.name.len() > 255
            || self.node.name.contains(['\0', '\r', '\n'])
        {
            return Err(ConfigError::Invalid(
                "node.name must be a bounded nonempty single-line value".into(),
            ));
        }
        if self.control_plane.connect_timeout_ms == 0
            || self.control_plane.connect_timeout_ms > 60_000
            || self.control_plane.request_timeout_ms == 0
            || self.control_plane.request_timeout_ms > 300_000
            || self.control_plane.artifact_transfer_timeout_ms < 1_000
            || self.control_plane.artifact_transfer_timeout_ms > 3_600_000
            || self.control_plane.long_poll_margin_ms == 0
            || self.control_plane.long_poll_margin_ms > 60_000
            || self.control_plane.retry_initial_ms == 0
            || self.control_plane.retry_max_ms < self.control_plane.retry_initial_ms
            || self.control_plane.retry_max_ms > 300_000
        {
            return Err(ConfigError::Invalid(
                "control-plane connection, request, artifact transfer, long-poll margin, and retry timings are independently bounded"
                    .into(),
            ));
        }
        if !(1024 * 1024..=10 * 1024 * 1024 * 1024_u64).contains(&self.artifacts.max_blob_bytes)
            || self.artifacts.max_entries == 0
            || self.artifacts.max_entries > 1_000_000
            || self.artifacts.max_file_bytes == 0
            || self.artifacts.max_file_bytes > self.artifacts.max_expanded_bytes
            || self.artifacts.max_expanded_bytes < self.artifacts.max_blob_bytes
            || self.artifacts.max_expanded_bytes > 20 * 1024 * 1024 * 1024_u64
        {
            return Err(ConfigError::Invalid(
                "artifacts requires a 1 MiB to 10 GiB blob bound, 1 to 1000000 entries, and ordered positive file/expanded bounds capped at 20 GiB"
                    .into(),
            ));
        }
        if !(1024 * 1024..=64 * 1024 * 1024).contains(&self.control_plane.max_response_bytes) {
            return Err(ConfigError::Invalid(
                "control_plane.max_response_bytes must be between 1 and 64 MiB".into(),
            ));
        }
        if self.logs.poll_interval_ms == 0
            || self.logs.poll_interval_ms > 60_000
            || self.logs.max_batch_chunks == 0
            || self.logs.max_batch_chunks > 256
            || !(1024 * 1024..=16 * 1024 * 1024).contains(&self.logs.max_batch_bytes)
        {
            return Err(ConfigError::Invalid(
                "logs polling and batch bounds are invalid".into(),
            ));
        }
        let Some(socket_path) = self.docker.socket.strip_prefix("unix://") else {
            return Err(ConfigError::Invalid(
                "docker.socket must be an absolute unix:// socket path".into(),
            ));
        };
        if !Path::new(socket_path).is_absolute()
            || socket_path.len() > 4096
            || socket_path.contains('\0')
        {
            return Err(ConfigError::Invalid(
                "docker.socket must be an absolute unix:// socket path".into(),
            ));
        }
        if !valid_docker_namespace(&self.docker.namespace)
            || self.docker.operation_timeout_ms == 0
            || self.docker.operation_timeout_ms > 900_000
        {
            return Err(ConfigError::Invalid(
                "docker namespace or operation timeout is invalid".into(),
            ));
        }
        validate_path("docker.secret_memory_dir", &self.docker.secret_memory_dir)?;
        if !self.docker.secret_memory_dir.is_absolute()
            || self
                .docker
                .secret_memory_dir
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(ConfigError::Invalid(
                "docker.secret_memory_dir must be an absolute normalized path".into(),
            ));
        }
        if !self
            .gateway
            .management_url
            .host_str()
            .is_some_and(is_loopback)
            || self.gateway.management_url.path() == "/"
        {
            return Err(ConfigError::Invalid(
                "gateway.management_url must be a node-local management API base URL".into(),
            ));
        }
        if !valid_env_name(&self.gateway.auth_token_env)
            || self.gateway.connect_timeout_ms == 0
            || self.gateway.connect_timeout_ms > 60_000
            || self.gateway.apply_timeout_ms == 0
            || self.gateway.apply_timeout_ms > 120_000
            || self.gateway.readiness_timeout_ms == 0
            || self.gateway.readiness_timeout_ms > 120_000
        {
            return Err(ConfigError::Invalid(
                "Gateway authentication environment variable or independent timeouts are invalid"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn enrollment_token(&self) -> Result<String, ConfigError> {
        let name = &self.control_plane.enrollment_token_env;
        let value = std::env::var(name).map_err(|_| {
            ConfigError::Invalid(format!("required environment variable {name:?} is not set"))
        })?;
        let Some(secret) = value.strip_prefix("a3sn_") else {
            return Err(ConfigError::Invalid(format!(
                "environment variable {name:?} is not a valid enrollment token"
            )));
        };
        if secret.len() != 64
            || !secret
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ConfigError::Invalid(format!(
                "environment variable {name:?} is not a valid enrollment token"
            )));
        }
        Ok(value)
    }

    pub fn gateway_auth_token(&self) -> Result<String, ConfigError> {
        let name = &self.gateway.auth_token_env;
        let value = std::env::var(name).map_err(|_| {
            ConfigError::Invalid(format!("required environment variable {name:?} is not set"))
        })?;
        if value.trim().is_empty() || value.len() > 4096 || value.contains(['\0', '\r', '\n']) {
            return Err(ConfigError::Invalid(format!(
                "environment variable {name:?} is not a valid Gateway management token"
            )));
        }
        Ok(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read node-agent config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid node-agent config: {0}")]
    Invalid(String),
}

fn validate_root(document: &Document) -> Result<(), ConfigError> {
    let allowed = [
        "artifacts",
        "control_plane",
        "docker",
        "gateway",
        "logs",
        "node",
    ];
    if document
        .blocks
        .iter()
        .any(|block| !allowed.contains(&block.name.as_str()))
    {
        return Err(ConfigError::Invalid(
            "config contains an unsupported root block".into(),
        ));
    }
    Ok(())
}

fn one_block<'a>(document: &'a Document, name: &str) -> Result<&'a Block, ConfigError> {
    let blocks = document
        .blocks
        .iter()
        .filter(|block| block.name == name)
        .collect::<Vec<_>>();
    if blocks.len() != 1 {
        return Err(ConfigError::Invalid(format!(
            "config must contain exactly one {name} block"
        )));
    }
    Ok(blocks[0])
}

fn validate_block(block: &Block, fields: &[&str]) -> Result<(), ConfigError> {
    if !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err(ConfigError::Invalid(format!(
            "{} block cannot contain labels or nested blocks",
            block.name
        )));
    }
    let expected = fields.iter().copied().collect::<BTreeSet<_>>();
    let actual = block
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(ConfigError::Invalid(format!(
            "{} block must contain exactly {}",
            block.name,
            fields.join(", ")
        )));
    }
    Ok(())
}

fn string(block: &Block, field: &str) -> Result<String, ConfigError> {
    block
        .attributes
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ConfigError::Invalid(format!("{}.{} must be a string", block.name, field)))
}

fn integer<T>(block: &Block, field: &str) -> Result<T, ConfigError>
where
    T: TryFrom<u64>,
{
    let number = block
        .attributes
        .get(field)
        .and_then(Value::as_number)
        .ok_or_else(|| {
            ConfigError::Invalid(format!("{}.{} must be an integer", block.name, field))
        })?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > u64::MAX as f64 {
        return Err(ConfigError::Invalid(format!(
            "{}.{} must be a nonnegative integer",
            block.name, field
        )));
    }
    T::try_from(number as u64)
        .map_err(|_| ConfigError::Invalid(format!("{}.{} is out of range", block.name, field)))
}

fn endpoint(
    label: &str,
    value: &str,
    allow_loopback_http: bool,
    require_root_path: bool,
) -> Result<Url, ConfigError> {
    let url = Url::parse(value)
        .map_err(|error| ConfigError::Invalid(format!("{label} is invalid: {error}")))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
        || require_root_path && url.path() != "/"
    {
        return Err(ConfigError::Invalid(format!(
            "{label} must be an absolute credential-free endpoint"
        )));
    }
    let secure = url.scheme() == "https";
    let development_loopback =
        allow_loopback_http && url.scheme() == "http" && url.host_str().is_some_and(is_loopback);
    if !secure && !development_loopback {
        return Err(ConfigError::Invalid(format!(
            "{label} must use HTTPS; HTTP is allowed only for loopback enrollment"
        )));
    }
    Ok(url)
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn validate_path(label: &str, value: &Path) -> Result<(), ConfigError> {
    let value = value.to_string_lossy();
    if value.trim().is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err(ConfigError::Invalid(format!("{label} is invalid")));
    }
    Ok(())
}

fn valid_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

pub(crate) fn valid_docker_namespace(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && !matches!(value, "." | "..")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
control_plane {
  enrollment_url = "http://127.0.0.1:8080/api/v1/node-control/enroll"
  node_control_url = "https://localhost:8443"
  enrollment_token_env = "A3S_CLOUD_ENROLLMENT_TOKEN"
  server_ca_file = ".a3s/cloud/security/node-ca/ca.pem"
  max_response_bytes = 20971520
  connect_timeout_ms = 5000
  request_timeout_ms = 10000
  artifact_transfer_timeout_ms = 900000
  long_poll_margin_ms = 5000
  retry_initial_ms = 250
  retry_max_ms = 30000
}

artifacts {
  max_blob_bytes = 1073741824
  max_entries = 100000
  max_file_bytes = 1073741824
  max_expanded_bytes = 4294967296
}

node {
  name = "worker-1"
  state_dir = ".a3s/cloud/node"
}

logs {
  poll_interval_ms = 1000
  max_batch_chunks = 256
  max_batch_bytes = 16777216
}

docker {
  socket = "unix:///var/run/docker.sock"
  namespace = "a3s-cloud"
  operation_timeout_ms = 120000
  secret_memory_dir = "/dev/shm/a3s-cloud/secrets"
}

gateway {
  management_url = "http://127.0.0.1:9090/api/gateway"
  auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  certificate_directory = "/var/lib/a3s-cloud/gateway/certificates"
  connect_timeout_ms = 5000
  apply_timeout_ms = 30000
  readiness_timeout_ms = 10000
}
"#;

    #[test]
    fn parses_a_closed_node_agent_configuration() {
        let config = NodeAgentConfig::parse(CONFIG).expect("node config");
        assert_eq!(config.node.name, "worker-1");
        assert_eq!(config.control_plane.node_control_url.scheme(), "https");
        assert_eq!(config.logs.max_batch_chunks, 256);
        assert_eq!(config.docker.namespace, "a3s-cloud");
        assert_eq!(config.gateway.management_url.path(), "/api/gateway");
        assert_eq!(
            config.gateway.certificate_directory,
            Path::new("/var/lib/a3s-cloud/gateway/certificates")
        );
    }

    #[test]
    fn loads_shipped_node_example_acl() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/node.example.acl");
        let config = NodeAgentConfig::load(&path)
            .unwrap_or_else(|error| panic!("failed to load {}: {error}", path.display()));

        assert_eq!(config.node.name, "worker-1");
        assert_eq!(config.control_plane.node_control_url.scheme(), "https");
        assert_eq!(config.logs.poll_interval_ms, 1000);
        assert_eq!(config.docker.namespace, "a3s-cloud");
        assert_eq!(config.gateway.management_url.path(), "/api/gateway");
    }

    #[test]
    fn rejects_unknown_fields_and_insecure_remote_enrollment() {
        let unknown = CONFIG.replace(
            "  retry_max_ms = 30000",
            "  retry_max_ms = 30000\n  fallback_provider = \"process\"",
        );
        assert!(NodeAgentConfig::parse(&unknown).is_err());
        let insecure = CONFIG.replace(
            "http://127.0.0.1:8080/api/v1/node-control/enroll",
            "http://cloud.example.com/api/v1/node-control/enroll",
        );
        assert!(NodeAgentConfig::parse(&insecure).is_err());
        let raw_provider = CONFIG.replace(
            "  name = \"worker-1\"",
            "  name = \"worker-1\"\n  provider = \"docker\"",
        );
        assert!(NodeAgentConfig::parse(&raw_provider).is_err());
        let parent_namespace =
            CONFIG.replace("  namespace = \"a3s-cloud\"", "  namespace = \"..\"");
        assert!(NodeAgentConfig::parse(&parent_namespace).is_err());
    }
}
