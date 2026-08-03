use a3s_acl::{Block, Document, Value};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use url::Url;

mod box_runtime;

pub use box_runtime::{
    BoxRuntimeConfig, BoxRuntimeIsolation, BoxRuntimeSevSnpConfig, BoxRuntimeSevSnpGeneration,
};

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
    pub box_runtime: BoxRuntimeConfig,
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
        let box_runtime = one_block(&document, "box")?;
        validate_box_block(
            box_runtime,
            &[
                "home_dir",
                "secret_root",
                "isolation",
                "control_timeout_ms",
                "task_poll_interval_ms",
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
            box_runtime: BoxRuntimeConfig {
                home_dir: PathBuf::from(string(box_runtime, "home_dir")?),
                secret_root: PathBuf::from(string(box_runtime, "secret_root")?),
                isolation: box_runtime::isolation(box_runtime)?,
                control_timeout_ms: integer(box_runtime, "control_timeout_ms")?,
                task_poll_interval_ms: integer(box_runtime, "task_poll_interval_ms")?,
                sev_snp: box_runtime::sev_snp(box_runtime)?,
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
        if !normalized_absolute_linux_directory(&self.gateway.certificate_directory) {
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
        validate_path("box.home_dir", &self.box_runtime.home_dir)?;
        if !normalized_absolute_linux_directory(&self.box_runtime.home_dir) {
            return Err(ConfigError::Invalid(
                "box.home_dir must be an absolute normalized directory".into(),
            ));
        }
        validate_path("box.secret_root", &self.box_runtime.secret_root)?;
        if !normalized_absolute_linux_directory(&self.box_runtime.secret_root) {
            return Err(ConfigError::Invalid(
                "box.secret_root must be an absolute normalized non-root Linux directory".into(),
            ));
        }
        if self.box_runtime.control_timeout_ms == 0
            || self.box_runtime.control_timeout_ms > 900_000
            || self.box_runtime.task_poll_interval_ms == 0
            || self.box_runtime.task_poll_interval_ms > 60_000
            || self.box_runtime.task_poll_interval_ms > self.box_runtime.control_timeout_ms
        {
            return Err(ConfigError::Invalid(
                "Box Runtime control timeout and Task poll interval are invalid".into(),
            ));
        }
        self.box_runtime.validate_sev_snp()?;
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
        "box",
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

fn validate_box_block(block: &Block, fields: &[&str]) -> Result<(), ConfigError> {
    if !block.labels.is_empty() {
        return Err(ConfigError::Invalid(
            "box block cannot contain labels".into(),
        ));
    }
    let expected = fields.iter().copied().collect::<BTreeSet<_>>();
    let actual = block
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(ConfigError::Invalid(format!(
            "box block must contain exactly {}",
            fields.join(", ")
        )));
    }
    if block.blocks.iter().any(|nested| nested.name != "sev_snp") {
        return Err(ConfigError::Invalid(
            "box block contains an unsupported nested block".into(),
        ));
    }
    if block
        .blocks
        .iter()
        .filter(|nested| nested.name == "sev_snp")
        .count()
        > 1
    {
        return Err(ConfigError::Invalid(
            "box.sev_snp block may appear only once".into(),
        ));
    }
    Ok(())
}

fn validate_optional_block(
    block: &Block,
    required: &[&str],
    optional: &[&str],
) -> Result<(), ConfigError> {
    if !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err(ConfigError::Invalid(format!(
            "{}.{} block cannot contain labels or nested blocks",
            "box", block.name
        )));
    }
    let allowed = required
        .iter()
        .chain(optional.iter())
        .copied()
        .collect::<BTreeSet<_>>();
    if block
        .attributes
        .keys()
        .any(|field| !allowed.contains(field.as_str()))
    {
        return Err(ConfigError::Invalid(format!(
            "box.{} block contains an unsupported attribute",
            block.name
        )));
    }
    if let Some(missing) = required
        .iter()
        .find(|field| !block.attributes.contains_key(**field))
    {
        return Err(ConfigError::Invalid(format!(
            "box.{}.{} is required",
            block.name, missing
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

fn optional_string(block: &Block, field: &str) -> Result<Option<String>, ConfigError> {
    block
        .attributes
        .get(field)
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                ConfigError::Invalid(format!("{}.{} must be a string", block.name, field))
            })
        })
        .transpose()
}

fn boolean(block: &Block, field: &str) -> Result<bool, ConfigError> {
    block
        .attributes
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| ConfigError::Invalid(format!("{}.{} must be a boolean", block.name, field)))
}

fn optional_integer<T>(block: &Block, field: &str) -> Result<Option<T>, ConfigError>
where
    T: TryFrom<u64>,
{
    if block.attributes.contains_key(field) {
        integer(block, field).map(Some)
    } else {
        Ok(None)
    }
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

fn normalized_absolute_linux_directory(value: &Path) -> bool {
    value.to_str().is_some_and(|value| {
        value.strip_prefix('/').is_some_and(|relative| {
            !relative.is_empty()
                && relative
                    .split('/')
                    .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
        }) && !value.contains([':', '\0'])
            && !value.bytes().any(|byte| byte.is_ascii_control())
    })
}

fn valid_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
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

box {
  home_dir = "/var/lib/a3s-box"
  secret_root = "/run/a3s-cloud/box-secrets"
  isolation = "microvm"
  control_timeout_ms = 120000
  task_poll_interval_ms = 50
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
        assert_eq!(config.box_runtime.home_dir, Path::new("/var/lib/a3s-box"));
        assert_eq!(
            config.box_runtime.secret_root,
            Path::new("/run/a3s-cloud/box-secrets")
        );
        assert_eq!(config.box_runtime.isolation, BoxRuntimeIsolation::Microvm);
        assert_eq!(config.box_runtime.sev_snp, None);
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
        assert_eq!(config.box_runtime.control_timeout_ms, 120000);
        assert_eq!(config.box_runtime.isolation, BoxRuntimeIsolation::Microvm);
        assert_eq!(config.box_runtime.sev_snp, None);
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
            "  name = \"worker-1\"\n  provider = \"a3s-box\"",
        );
        assert!(NodeAgentConfig::parse(&raw_provider).is_err());
        let parent_home = CONFIG.replace(
            "  home_dir = \"/var/lib/a3s-box\"",
            "  home_dir = \"/var/lib/../a3s-box\"",
        );
        assert!(NodeAgentConfig::parse(&parent_home).is_err());
        let non_normal_secret_root = CONFIG.replace(
            "  secret_root = \"/run/a3s-cloud/box-secrets\"",
            "  secret_root = \"/run/a3s-cloud/../box-secrets\"",
        );
        assert!(NodeAgentConfig::parse(&non_normal_secret_root).is_err());
        let root_secret_root = CONFIG.replace(
            "  secret_root = \"/run/a3s-cloud/box-secrets\"",
            "  secret_root = \"/\"",
        );
        assert!(NodeAgentConfig::parse(&root_secret_root).is_err());
        let implicit_fallback =
            CONFIG.replace("  isolation = \"microvm\"", "  isolation = \"automatic\"");
        assert!(NodeAgentConfig::parse(&implicit_fallback).is_err());
        let missing_isolation = CONFIG.replace("  isolation = \"microvm\"\n", "");
        assert!(NodeAgentConfig::parse(&missing_isolation).is_err());
        let missing_secret_root =
            CONFIG.replace("  secret_root = \"/run/a3s-cloud/box-secrets\"\n", "");
        assert!(NodeAgentConfig::parse(&missing_secret_root).is_err());
    }

    #[test]
    fn parses_explicit_box_sandbox_without_changing_the_default_example() {
        let sandbox = CONFIG.replace("  isolation = \"microvm\"", "  isolation = \"sandbox\"");
        let config = NodeAgentConfig::parse(&sandbox).expect("Sandbox node config");

        assert_eq!(config.box_runtime.isolation, BoxRuntimeIsolation::Sandbox);
    }

    #[test]
    fn parses_explicit_hardware_sev_snp_policy() {
        let measurement = "ab".repeat(48);
        let source = with_sev_snp(&format!(
            r#"    generation = "genoa"
    simulate = false
    expected_measurement = "{measurement}"
    require_no_debug = true
    require_no_smt = true
    allowed_policy_mask = 112
    min_boot_loader_svn = 3
    min_tee_svn = 4
    min_snp_svn = 5
    min_microcode_svn = 6"#
        ));
        let config = NodeAgentConfig::parse(&source).expect("hardware SEV-SNP node config");
        let sev_snp = config.box_runtime.sev_snp.expect("explicit SEV-SNP config");

        assert_eq!(sev_snp.generation, BoxRuntimeSevSnpGeneration::Genoa);
        assert!(!sev_snp.simulate);
        assert_eq!(
            sev_snp.expected_measurement.as_deref(),
            Some(measurement.as_str())
        );
        assert!(sev_snp.require_no_debug);
        assert!(sev_snp.require_no_smt);
        assert_eq!(sev_snp.allowed_policy_mask, Some(112));
        assert_eq!(sev_snp.min_boot_loader_svn, Some(3));
        assert_eq!(sev_snp.min_tee_svn, Some(4));
        assert_eq!(sev_snp.min_snp_svn, Some(5));
        assert_eq!(sev_snp.min_microcode_svn, Some(6));
    }

    #[test]
    fn parses_explicit_simulated_sev_snp_without_a_measurement() {
        let source = with_sev_snp(
            r#"    generation = "milan"
    simulate = true
    require_no_debug = false
    require_no_smt = false"#,
        );
        let config = NodeAgentConfig::parse(&source).expect("simulated SEV-SNP node config");
        let sev_snp = config
            .box_runtime
            .sev_snp
            .expect("explicit simulated SEV-SNP config");

        assert_eq!(sev_snp.generation, BoxRuntimeSevSnpGeneration::Milan);
        assert!(sev_snp.simulate);
        assert_eq!(sev_snp.expected_measurement, None);
    }

    #[test]
    fn rejects_ambiguous_or_unsupported_sev_snp_blocks() {
        let valid = r#"    generation = "milan"
    simulate = true
    require_no_debug = true
    require_no_smt = false"#;
        let duplicate = with_sev_snp(&format!("{valid}\n  }}\n\n  sev_snp {{\n{valid}"));
        assert!(NodeAgentConfig::parse(&duplicate).is_err());

        let labeled = with_sev_snp(valid).replacen("sev_snp {", "sev_snp \"prod\" {", 1);
        assert!(NodeAgentConfig::parse(&labeled).is_err());

        let unknown_attribute = with_sev_snp(&format!("{valid}\n    provider = \"automatic\""));
        assert!(NodeAgentConfig::parse(&unknown_attribute).is_err());

        let unknown_block = CONFIG.replacen(
            "  task_poll_interval_ms = 50\n}",
            "  task_poll_interval_ms = 50\n\n  tdx {\n  }\n}",
            1,
        );
        assert!(NodeAgentConfig::parse(&unknown_block).is_err());
    }

    #[test]
    fn rejects_unsafe_hardware_sev_snp_policy() {
        let base = r#"    generation = "milan"
    simulate = false
    require_no_debug = true
    require_no_smt = false"#;
        assert!(NodeAgentConfig::parse(&with_sev_snp(base)).is_err());

        let uppercase_measurement = with_sev_snp(&format!(
            "{base}\n    expected_measurement = \"{}\"",
            "AB".repeat(48)
        ));
        assert!(NodeAgentConfig::parse(&uppercase_measurement).is_err());

        let debug = with_sev_snp(&format!(
            "{}\n    expected_measurement = \"{}\"",
            base.replace("require_no_debug = true", "require_no_debug = false"),
            "ab".repeat(48)
        ));
        assert!(NodeAgentConfig::parse(&debug).is_err());

        let sandbox = with_sev_snp(&format!(
            "{base}\n    expected_measurement = \"{}\"",
            "ab".repeat(48)
        ))
        .replace("  isolation = \"microvm\"", "  isolation = \"sandbox\"");
        assert!(NodeAgentConfig::parse(&sandbox).is_err());

        let inexact_mask = with_sev_snp(
            r#"    generation = "milan"
    simulate = true
    require_no_debug = true
    require_no_smt = false
    allowed_policy_mask = 9007199254740992"#,
        );
        assert!(NodeAgentConfig::parse(&inexact_mask).is_err());
    }

    fn with_sev_snp(body: &str) -> String {
        CONFIG.replacen(
            "  task_poll_interval_ms = 50\n}",
            &format!("  task_poll_interval_ms = 50\n\n  sev_snp {{\n{body}\n  }}\n}}"),
            1,
        )
    }
}
