use crate::modules::sources::domain::{GitProvider, GitRepository, SourceRepositoryPolicy};
use a3s_acl::{Block, Document, Value};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRole {
    All,
    Api,
    Worker,
    Relay,
}

impl ProcessRole {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "all" => Ok(Self::All),
            "api" => Ok(Self::Api),
            "worker" => Ok(Self::Worker),
            "relay" => Ok(Self::Relay),
            _ => Err(ConfigError::Invalid(format!(
                "server.role {value:?} must be all, api, worker, or relay"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub role: ProcessRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeControlConfig {
    pub host: String,
    pub port: u16,
    pub server_name: String,
    pub certificate_file: String,
    pub private_key_file: String,
    pub client_ca_file: String,
    pub max_request_bytes: usize,
    pub tls_handshake_timeout_ms: u64,
    pub request_body_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactTransferConfig {
    pub store_dir: String,
    pub max_blob_bytes: u64,
    pub transfer_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresConfig {
    pub url_env: String,
    pub max_connections: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub bootstrap_token_env: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventProviderKind {
    Memory,
    Nats,
}

impl EventProviderKind {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "memory" => Ok(Self::Memory),
            "nats" => Ok(Self::Nats),
            _ => Err(ConfigError::Invalid(format!(
                "events.provider {value:?} must be memory or nats"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventsConfig {
    pub provider: EventProviderKind,
    pub nats_url_env: String,
    pub stream_name: String,
    pub batch_size: usize,
    pub poll_interval_ms: u64,
    pub lease_ms: u64,
    pub publish_timeout_ms: u64,
    pub retry_initial_ms: u64,
    pub retry_max_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationsConfig {
    pub reconcile_interval_ms: u64,
    pub lease_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentsConfig {
    pub reconcile_interval_ms: u64,
    pub command_ttl_ms: u64,
    pub runtime_apply_timeout_ms: u64,
    pub observation_poll_ms: u64,
    pub convergence_timeout_ms: u64,
    pub runtime_stop_timeout_ms: u64,
    pub cleanup_poll_ms: u64,
    pub cleanup_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryConfig {
    pub request_timeout_ms: u64,
    pub insecure_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcesConfig {
    pub github_request_timeout_ms: u64,
    pub github_webhook_secret_env: String,
    pub github_webhook_max_body_bytes: usize,
    pub github_app_enabled: bool,
    pub github_app_slug: String,
    pub github_app_client_id: String,
    pub github_app_client_secret_env: String,
    pub github_app_private_key_env: String,
    pub github_app_callback_url: String,
    pub github_connection_state_ttl_ms: u64,
    pub allowed_repositories: Vec<String>,
    pub denied_repositories: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStorageProviderKind {
    Local,
    S3,
}

impl LogStorageProviderKind {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "local" => Ok(Self::Local),
            "s3" => Ok(Self::S3),
            _ => Err(ConfigError::Invalid(format!(
                "logs.storage_provider {value:?} must be local or s3"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogsConfig {
    pub storage_provider: LogStorageProviderKind,
    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_prefix: String,
    pub s3_access_key_env: String,
    pub s3_secret_key_env: String,
    pub s3_session_token_env: String,
    pub s3_allow_http: bool,
    pub s3_virtual_hosted_style: bool,
    pub s3_request_timeout_ms: u64,
    pub s3_connect_timeout_ms: u64,
    pub s3_retry_timeout_ms: u64,
    pub s3_max_retries: usize,
    pub retention_ms: u64,
    pub retention_poll_ms: u64,
    pub retention_batch_size: usize,
    pub tombstone_retention_ms: u64,
    pub tombstone_compaction_poll_ms: u64,
    pub tombstone_compaction_batch_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeConfig {
    pub entrypoint_address: String,
    pub management_address: String,
    pub management_path_prefix: String,
    pub management_auth_token_env: String,
    pub domain_verification_timeout_ms: u64,
    pub certificate_directory: String,
    pub certificate_ttl_ms: u64,
    pub certificate_renewal_window_ms: u64,
    pub certificate_reconciliation_interval_ms: u64,
    pub upstream_request_timeout_ms: u64,
    pub command_ttl_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetConfig {
    pub heartbeat_interval_ms: u64,
    pub heartbeat_timeout_ms: u64,
    pub command_long_poll_ms: u64,
    pub command_lease_ms: u64,
    pub certificate_ttl_ms: u64,
    pub certificate_rotation_window_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityProfile {
    Development,
    Production,
}

impl SecurityProfile {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "development" => Ok(Self::Development),
            "production" => Ok(Self::Production),
            _ => Err(ConfigError::Invalid(format!(
                "security.profile {value:?} must be development or production"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityProviderKind {
    Local,
    Vault,
}

impl SecurityProviderKind {
    fn parse(field: &str, value: &str) -> Result<Self, ConfigError> {
        match value {
            "local" => Ok(Self::Local),
            "vault" => Ok(Self::Vault),
            _ => Err(ConfigError::Invalid(format!(
                "security.{field} {value:?} must be local or vault"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityConfig {
    pub profile: SecurityProfile,
    pub state_dir: String,
    pub certificate_authority: SecurityProviderKind,
    pub gateway_certificate_authority: SecurityProviderKind,
    pub key_encryption: SecurityProviderKind,
    pub vault_address_env: String,
    pub vault_token_env: String,
    pub vault_pki_mount: String,
    pub vault_pki_role: String,
    pub vault_gateway_pki_mount: String,
    pub vault_gateway_pki_role: String,
    pub vault_transit_mount: String,
    pub vault_transit_key: String,
    pub vault_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudConfig {
    pub server: ServerConfig,
    pub node_control: NodeControlConfig,
    pub artifacts: ArtifactTransferConfig,
    pub postgres: PostgresConfig,
    pub auth: AuthConfig,
    pub events: EventsConfig,
    pub operations: OperationsConfig,
    pub deployments: DeploymentsConfig,
    pub registry: RegistryConfig,
    pub sources: SourcesConfig,
    pub logs: LogsConfig,
    pub edge: EdgeConfig,
    pub fleet: FleetConfig,
    pub security: SecurityConfig,
}

impl CloudConfig {
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
        let server = one_block(&document, "server")?;
        validate_block(server, &["host", "port", "role"])?;
        let node_control = one_block(&document, "node_control")?;
        validate_block(
            node_control,
            &[
                "host",
                "port",
                "server_name",
                "certificate_file",
                "private_key_file",
                "client_ca_file",
                "max_request_bytes",
                "tls_handshake_timeout_ms",
                "request_body_timeout_ms",
            ],
        )?;
        let artifacts = one_block(&document, "artifacts")?;
        validate_block(
            artifacts,
            &["store_dir", "max_blob_bytes", "transfer_timeout_ms"],
        )?;
        let postgres = one_block(&document, "postgres")?;
        validate_block(postgres, &["url_env", "max_connections"])?;
        let auth = one_block(&document, "auth")?;
        validate_block(auth, &["bootstrap_token_env"])?;
        let events = one_block(&document, "events")?;
        validate_block(
            events,
            &[
                "provider",
                "nats_url_env",
                "stream_name",
                "batch_size",
                "poll_interval_ms",
                "lease_ms",
                "publish_timeout_ms",
                "retry_initial_ms",
                "retry_max_ms",
            ],
        )?;
        let operations = one_block(&document, "operations")?;
        validate_block(operations, &["reconcile_interval_ms", "lease_ms"])?;
        let deployments = one_block(&document, "deployments")?;
        validate_block(
            deployments,
            &[
                "command_ttl_ms",
                "reconcile_interval_ms",
                "runtime_apply_timeout_ms",
                "observation_poll_ms",
                "convergence_timeout_ms",
                "runtime_stop_timeout_ms",
                "cleanup_poll_ms",
                "cleanup_timeout_ms",
            ],
        )?;
        let registry = one_block(&document, "registry")?;
        validate_block(registry, &["request_timeout_ms", "insecure_hosts"])?;
        let sources = one_block(&document, "sources")?;
        validate_block(
            sources,
            &[
                "github_request_timeout_ms",
                "github_webhook_secret_env",
                "github_webhook_max_body_bytes",
                "github_app_enabled",
                "github_app_slug",
                "github_app_client_id",
                "github_app_client_secret_env",
                "github_app_private_key_env",
                "github_app_callback_url",
                "github_connection_state_ttl_ms",
                "allowed_repositories",
                "denied_repositories",
            ],
        )?;
        let logs = one_block(&document, "logs")?;
        validate_block(
            logs,
            &[
                "storage_provider",
                "s3_endpoint",
                "s3_region",
                "s3_bucket",
                "s3_prefix",
                "s3_access_key_env",
                "s3_secret_key_env",
                "s3_session_token_env",
                "s3_allow_http",
                "s3_virtual_hosted_style",
                "s3_request_timeout_ms",
                "s3_connect_timeout_ms",
                "s3_retry_timeout_ms",
                "s3_max_retries",
                "retention_ms",
                "retention_poll_ms",
                "retention_batch_size",
                "tombstone_retention_ms",
                "tombstone_compaction_poll_ms",
                "tombstone_compaction_batch_size",
            ],
        )?;
        let edge = one_block(&document, "edge")?;
        validate_block(
            edge,
            &[
                "entrypoint_address",
                "management_address",
                "management_path_prefix",
                "management_auth_token_env",
                "domain_verification_timeout_ms",
                "certificate_directory",
                "certificate_ttl_ms",
                "certificate_renewal_window_ms",
                "certificate_reconciliation_interval_ms",
                "upstream_request_timeout_ms",
                "command_ttl_ms",
            ],
        )?;
        let fleet = one_block(&document, "fleet")?;
        validate_block(
            fleet,
            &[
                "heartbeat_interval_ms",
                "heartbeat_timeout_ms",
                "command_long_poll_ms",
                "command_lease_ms",
                "certificate_ttl_ms",
                "certificate_rotation_window_ms",
            ],
        )?;
        let security = one_block(&document, "security")?;
        validate_block(
            security,
            &[
                "profile",
                "state_dir",
                "certificate_authority",
                "gateway_certificate_authority",
                "key_encryption",
                "vault_address_env",
                "vault_token_env",
                "vault_pki_mount",
                "vault_pki_role",
                "vault_gateway_pki_mount",
                "vault_gateway_pki_role",
                "vault_transit_mount",
                "vault_transit_key",
                "vault_timeout_ms",
            ],
        )?;

        let config = Self {
            server: ServerConfig {
                host: string(server, "host")?,
                port: integer(server, "port")?,
                role: ProcessRole::parse(&string(server, "role")?)?,
            },
            node_control: NodeControlConfig {
                host: string(node_control, "host")?,
                port: integer(node_control, "port")?,
                server_name: string(node_control, "server_name")?,
                certificate_file: string(node_control, "certificate_file")?,
                private_key_file: string(node_control, "private_key_file")?,
                client_ca_file: string(node_control, "client_ca_file")?,
                max_request_bytes: integer(node_control, "max_request_bytes")?,
                tls_handshake_timeout_ms: integer(node_control, "tls_handshake_timeout_ms")?,
                request_body_timeout_ms: integer(node_control, "request_body_timeout_ms")?,
            },
            artifacts: ArtifactTransferConfig {
                store_dir: string(artifacts, "store_dir")?,
                max_blob_bytes: integer(artifacts, "max_blob_bytes")?,
                transfer_timeout_ms: integer(artifacts, "transfer_timeout_ms")?,
            },
            postgres: PostgresConfig {
                url_env: string(postgres, "url_env")?,
                max_connections: integer(postgres, "max_connections")?,
            },
            auth: AuthConfig {
                bootstrap_token_env: string(auth, "bootstrap_token_env")?,
            },
            events: EventsConfig {
                provider: EventProviderKind::parse(&string(events, "provider")?)?,
                nats_url_env: string(events, "nats_url_env")?,
                stream_name: string(events, "stream_name")?,
                batch_size: integer(events, "batch_size")?,
                poll_interval_ms: integer(events, "poll_interval_ms")?,
                lease_ms: integer(events, "lease_ms")?,
                publish_timeout_ms: integer(events, "publish_timeout_ms")?,
                retry_initial_ms: integer(events, "retry_initial_ms")?,
                retry_max_ms: integer(events, "retry_max_ms")?,
            },
            operations: OperationsConfig {
                reconcile_interval_ms: integer(operations, "reconcile_interval_ms")?,
                lease_ms: integer(operations, "lease_ms")?,
            },
            deployments: DeploymentsConfig {
                reconcile_interval_ms: integer(deployments, "reconcile_interval_ms")?,
                command_ttl_ms: integer(deployments, "command_ttl_ms")?,
                runtime_apply_timeout_ms: integer(deployments, "runtime_apply_timeout_ms")?,
                observation_poll_ms: integer(deployments, "observation_poll_ms")?,
                convergence_timeout_ms: integer(deployments, "convergence_timeout_ms")?,
                runtime_stop_timeout_ms: integer(deployments, "runtime_stop_timeout_ms")?,
                cleanup_poll_ms: integer(deployments, "cleanup_poll_ms")?,
                cleanup_timeout_ms: integer(deployments, "cleanup_timeout_ms")?,
            },
            registry: RegistryConfig {
                request_timeout_ms: integer(registry, "request_timeout_ms")?,
                insecure_hosts: string_list(registry, "insecure_hosts")?,
            },
            sources: SourcesConfig {
                github_request_timeout_ms: integer(sources, "github_request_timeout_ms")?,
                github_webhook_secret_env: string(sources, "github_webhook_secret_env")?,
                github_webhook_max_body_bytes: integer(sources, "github_webhook_max_body_bytes")?,
                github_app_enabled: boolean(sources, "github_app_enabled")?,
                github_app_slug: string(sources, "github_app_slug")?,
                github_app_client_id: string(sources, "github_app_client_id")?,
                github_app_client_secret_env: string(sources, "github_app_client_secret_env")?,
                github_app_private_key_env: string(sources, "github_app_private_key_env")?,
                github_app_callback_url: string(sources, "github_app_callback_url")?,
                github_connection_state_ttl_ms: integer(sources, "github_connection_state_ttl_ms")?,
                allowed_repositories: string_list(sources, "allowed_repositories")?,
                denied_repositories: string_list(sources, "denied_repositories")?,
            },
            logs: LogsConfig {
                storage_provider: LogStorageProviderKind::parse(&string(
                    logs,
                    "storage_provider",
                )?)?,
                s3_endpoint: string(logs, "s3_endpoint")?,
                s3_region: string(logs, "s3_region")?,
                s3_bucket: string(logs, "s3_bucket")?,
                s3_prefix: string(logs, "s3_prefix")?,
                s3_access_key_env: string(logs, "s3_access_key_env")?,
                s3_secret_key_env: string(logs, "s3_secret_key_env")?,
                s3_session_token_env: string(logs, "s3_session_token_env")?,
                s3_allow_http: boolean(logs, "s3_allow_http")?,
                s3_virtual_hosted_style: boolean(logs, "s3_virtual_hosted_style")?,
                s3_request_timeout_ms: integer(logs, "s3_request_timeout_ms")?,
                s3_connect_timeout_ms: integer(logs, "s3_connect_timeout_ms")?,
                s3_retry_timeout_ms: integer(logs, "s3_retry_timeout_ms")?,
                s3_max_retries: integer(logs, "s3_max_retries")?,
                retention_ms: integer(logs, "retention_ms")?,
                retention_poll_ms: integer(logs, "retention_poll_ms")?,
                retention_batch_size: integer(logs, "retention_batch_size")?,
                tombstone_retention_ms: integer(logs, "tombstone_retention_ms")?,
                tombstone_compaction_poll_ms: integer(logs, "tombstone_compaction_poll_ms")?,
                tombstone_compaction_batch_size: integer(logs, "tombstone_compaction_batch_size")?,
            },
            edge: EdgeConfig {
                entrypoint_address: string(edge, "entrypoint_address")?,
                management_address: string(edge, "management_address")?,
                management_path_prefix: string(edge, "management_path_prefix")?,
                management_auth_token_env: string(edge, "management_auth_token_env")?,
                domain_verification_timeout_ms: integer(edge, "domain_verification_timeout_ms")?,
                certificate_directory: string(edge, "certificate_directory")?,
                certificate_ttl_ms: integer(edge, "certificate_ttl_ms")?,
                certificate_renewal_window_ms: integer(edge, "certificate_renewal_window_ms")?,
                certificate_reconciliation_interval_ms: integer(
                    edge,
                    "certificate_reconciliation_interval_ms",
                )?,
                upstream_request_timeout_ms: integer(edge, "upstream_request_timeout_ms")?,
                command_ttl_ms: integer(edge, "command_ttl_ms")?,
            },
            fleet: FleetConfig {
                heartbeat_interval_ms: integer(fleet, "heartbeat_interval_ms")?,
                heartbeat_timeout_ms: integer(fleet, "heartbeat_timeout_ms")?,
                command_long_poll_ms: integer(fleet, "command_long_poll_ms")?,
                command_lease_ms: integer(fleet, "command_lease_ms")?,
                certificate_ttl_ms: integer(fleet, "certificate_ttl_ms")?,
                certificate_rotation_window_ms: integer(fleet, "certificate_rotation_window_ms")?,
            },
            security: SecurityConfig {
                profile: SecurityProfile::parse(&string(security, "profile")?)?,
                state_dir: string(security, "state_dir")?,
                certificate_authority: SecurityProviderKind::parse(
                    "certificate_authority",
                    &string(security, "certificate_authority")?,
                )?,
                gateway_certificate_authority: SecurityProviderKind::parse(
                    "gateway_certificate_authority",
                    &string(security, "gateway_certificate_authority")?,
                )?,
                key_encryption: SecurityProviderKind::parse(
                    "key_encryption",
                    &string(security, "key_encryption")?,
                )?,
                vault_address_env: string(security, "vault_address_env")?,
                vault_token_env: string(security, "vault_token_env")?,
                vault_pki_mount: string(security, "vault_pki_mount")?,
                vault_pki_role: string(security, "vault_pki_role")?,
                vault_gateway_pki_mount: string(security, "vault_gateway_pki_mount")?,
                vault_gateway_pki_role: string(security, "vault_gateway_pki_role")?,
                vault_transit_mount: string(security, "vault_transit_mount")?,
                vault_transit_key: string(security, "vault_transit_key")?,
                vault_timeout_ms: integer(security, "vault_timeout_ms")?,
            },
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.server.host.trim().is_empty() || self.server.host.len() > 255 {
            return Err(ConfigError::Invalid(
                "server.host must be a bounded nonempty value".into(),
            ));
        }
        if self.server.port == 0 {
            return Err(ConfigError::Invalid(
                "server.port must be greater than zero".into(),
            ));
        }
        if self.node_control.host.trim().is_empty()
            || self.node_control.host.len() > 255
            || self.node_control.port == 0
            || self.node_control.server_name.trim().is_empty()
            || self.node_control.server_name.len() > 255
            || self.node_control.max_request_bytes < 1024 * 1024
            || self.node_control.max_request_bytes > 64 * 1024 * 1024
            || self.node_control.tls_handshake_timeout_ms == 0
            || self.node_control.tls_handshake_timeout_ms > 60_000
            || self.node_control.request_body_timeout_ms == 0
            || self.node_control.request_body_timeout_ms > 60_000
        {
            return Err(ConfigError::Invalid(
                "node_control requires a valid address, server name, 1-64 MiB request bound, and independent 1-60000 ms TLS handshake and request body timeouts"
                    .into(),
            ));
        }
        for (label, value) in [
            ("certificate_file", &self.node_control.certificate_file),
            ("private_key_file", &self.node_control.private_key_file),
            ("client_ca_file", &self.node_control.client_ca_file),
        ] {
            if value.trim().is_empty() || value.len() > 4096 || value.contains('\0') {
                return Err(ConfigError::Invalid(format!(
                    "node_control.{label} is invalid"
                )));
            }
        }
        if self.node_control.certificate_file == self.node_control.private_key_file {
            return Err(ConfigError::Invalid(
                "node_control certificate and private key files must differ".into(),
            ));
        }
        let artifact_path = Path::new(&self.artifacts.store_dir);
        if self.artifacts.store_dir.trim().is_empty()
            || self.artifacts.store_dir.len() > 4096
            || self.artifacts.store_dir.contains('\0')
            || artifact_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            || !(1024 * 1024..=10 * 1024 * 1024 * 1024_u64).contains(&self.artifacts.max_blob_bytes)
            || !(1_000..=3_600_000).contains(&self.artifacts.transfer_timeout_ms)
        {
            return Err(ConfigError::Invalid(
                "artifacts requires a normalized store path, a 1 MiB to 10 GiB blob bound, and a 1 second to 1 hour transfer timeout"
                    .into(),
            ));
        }
        if !valid_env_name(&self.postgres.url_env) {
            return Err(ConfigError::Invalid(
                "postgres.url_env must be an uppercase environment variable name".into(),
            ));
        }
        if self.postgres.max_connections == 0 || self.postgres.max_connections > 1024 {
            return Err(ConfigError::Invalid(
                "postgres.max_connections must be between 1 and 1024".into(),
            ));
        }
        if !valid_env_name(&self.auth.bootstrap_token_env) {
            return Err(ConfigError::Invalid(
                "auth.bootstrap_token_env must be an uppercase environment variable name".into(),
            ));
        }
        if !valid_env_name(&self.events.nats_url_env) {
            return Err(ConfigError::Invalid(
                "events.nats_url_env must be an uppercase environment variable name".into(),
            ));
        }
        if self.events.stream_name.is_empty()
            || self.events.stream_name.len() > 63
            || !self
                .events
                .stream_name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(ConfigError::Invalid(
                "events.stream_name must contain 1 to 63 uppercase letters, digits, or underscores"
                    .into(),
            ));
        }
        if self.events.batch_size == 0
            || self.events.batch_size > 10_000
            || self.events.poll_interval_ms == 0
            || self.events.publish_timeout_ms == 0
            || self.events.lease_ms <= self.events.publish_timeout_ms
            || self.events.retry_initial_ms == 0
            || self.events.retry_max_ms < self.events.retry_initial_ms
        {
            return Err(ConfigError::Invalid(
                "events relay requires a batch of 1 to 10000, positive independent timings, a lease longer than publish timeout, and ordered retry bounds"
                    .into(),
            ));
        }
        if self.operations.reconcile_interval_ms == 0
            || self.operations.lease_ms <= self.operations.reconcile_interval_ms
        {
            return Err(ConfigError::Invalid(
                "operations.lease_ms must exceed a positive reconcile interval".into(),
            ));
        }
        if [
            self.deployments.reconcile_interval_ms,
            self.deployments.command_ttl_ms,
            self.deployments.runtime_apply_timeout_ms,
            self.deployments.observation_poll_ms,
            self.deployments.convergence_timeout_ms,
            self.deployments.runtime_stop_timeout_ms,
            self.deployments.cleanup_poll_ms,
            self.deployments.cleanup_timeout_ms,
        ]
        .contains(&0)
        {
            return Err(ConfigError::Invalid(
                "deployment reconciliation, command, Runtime apply, convergence, Runtime stop, cleanup poll, and cleanup deadlines must each be positive"
                    .into(),
            ));
        }
        if self.registry.request_timeout_ms == 0
            || self.registry.request_timeout_ms > 60_000
            || self.registry.insecure_hosts.len() > 64
            || self.registry.insecure_hosts.iter().any(|host| {
                host.is_empty()
                    || host.len() > 255
                    || host.contains(['/', '@', '\\', '\0', '\r', '\n', ' ', '\t'])
            })
        {
            return Err(ConfigError::Invalid(
                "registry requires a 1-60000 ms request timeout and at most 64 explicit insecure host[:port] values"
                    .into(),
            ));
        }
        let mut unique_registry_hosts = self.registry.insecure_hosts.clone();
        unique_registry_hosts.sort();
        unique_registry_hosts.dedup();
        if unique_registry_hosts.len() != self.registry.insecure_hosts.len() {
            return Err(ConfigError::Invalid(
                "registry.insecure_hosts cannot contain duplicates".into(),
            ));
        }
        if self.sources.github_request_timeout_ms == 0
            || self.sources.github_request_timeout_ms > 60_000
            || !(1024..=2 * 1024 * 1024).contains(&self.sources.github_webhook_max_body_bytes)
            || !(60_000..=1_800_000).contains(&self.sources.github_connection_state_ttl_ms)
            || self.sources.allowed_repositories.len() > 256
            || self.sources.denied_repositories.len() > 256
        {
            return Err(ConfigError::Invalid(
                "sources requires a 1-60000 ms GitHub request timeout, a 1024-byte to 2-MiB webhook body limit, a 1-30 minute connection-state TTL, and at most 256 exact allowlisted and denied repositories"
                    .into(),
            ));
        }
        if !valid_env_name(&self.sources.github_webhook_secret_env) {
            return Err(ConfigError::Invalid(
                "sources.github_webhook_secret_env must be an uppercase environment variable name"
                    .into(),
            ));
        }
        self.validate_github_app()?;
        SourceRepositoryPolicy::github(
            &self.sources.allowed_repositories,
            &self.sources.denied_repositories,
        )
        .map_err(|error| ConfigError::Invalid(format!("sources policy is invalid: {error}")))?;
        validate_unique_repositories(
            "sources.allowed_repositories",
            &self.sources.allowed_repositories,
        )?;
        validate_unique_repositories(
            "sources.denied_repositories",
            &self.sources.denied_repositories,
        )?;
        if !(60_000..=315_576_000_000).contains(&self.logs.retention_ms)
            || self.logs.retention_poll_ms == 0
            || self.logs.retention_poll_ms > 86_400_000
            || self.logs.retention_poll_ms > self.logs.retention_ms
            || self.logs.retention_batch_size == 0
            || self.logs.retention_batch_size > 10_000
        {
            return Err(ConfigError::Invalid(
                "logs retention must be 1 minute to 10 years with a bounded poll interval and batch of 1 to 10000"
                    .into(),
            ));
        }
        if !(60_000..=315_576_000_000).contains(&self.logs.tombstone_retention_ms)
            || self.logs.tombstone_compaction_poll_ms == 0
            || self.logs.tombstone_compaction_poll_ms > 86_400_000
            || self.logs.tombstone_compaction_poll_ms > self.logs.tombstone_retention_ms
            || self.logs.tombstone_compaction_batch_size == 0
            || self.logs.tombstone_compaction_batch_size > 10_000
        {
            return Err(ConfigError::Invalid(
                "logs tombstones must be retained for 1 minute to 10 years before compaction with a bounded poll interval and batch of 1 to 10000"
                    .into(),
            ));
        }
        for (label, value) in [
            ("s3_access_key_env", &self.logs.s3_access_key_env),
            ("s3_secret_key_env", &self.logs.s3_secret_key_env),
        ] {
            if !valid_env_name(value) {
                return Err(ConfigError::Invalid(format!(
                    "logs.{label} must be an uppercase environment variable name"
                )));
            }
        }
        if !self.logs.s3_session_token_env.is_empty()
            && !valid_env_name(&self.logs.s3_session_token_env)
        {
            return Err(ConfigError::Invalid(
                "logs.s3_session_token_env must be empty or an uppercase environment variable name"
                    .into(),
            ));
        }
        if !valid_s3_region(&self.logs.s3_region)
            || !valid_s3_bucket(&self.logs.s3_bucket)
            || !valid_object_prefix(&self.logs.s3_prefix)
            || !valid_s3_endpoint(&self.logs.s3_endpoint, self.logs.s3_allow_http)
            || self.logs.s3_request_timeout_ms == 0
            || self.logs.s3_request_timeout_ms > 300_000
            || self.logs.s3_connect_timeout_ms == 0
            || self.logs.s3_connect_timeout_ms > 60_000
            || self.logs.s3_connect_timeout_ms > self.logs.s3_request_timeout_ms
            || self.logs.s3_retry_timeout_ms < self.logs.s3_request_timeout_ms
            || self.logs.s3_retry_timeout_ms > 300_000
            || self.logs.s3_max_retries > 10
        {
            return Err(ConfigError::Invalid(
                "logs S3 storage requires a safe endpoint, region, bucket, prefix, 1-300000 ms request/retry bounds, a connect timeout no longer than the request timeout, and at most 10 retries"
                    .into(),
            ));
        }
        let entrypoint = self
            .edge
            .entrypoint_address
            .parse::<SocketAddr>()
            .map_err(|error| {
                ConfigError::Invalid(format!("edge.entrypoint_address is invalid: {error}"))
            })?;
        let management = self
            .edge
            .management_address
            .parse::<SocketAddr>()
            .map_err(|error| {
                ConfigError::Invalid(format!("edge.management_address is invalid: {error}"))
            })?;
        let certificate_directory = std::path::Path::new(&self.edge.certificate_directory);
        if entrypoint.port() == 0
            || management.port() == 0
            || !management.ip().is_loopback()
            || !self.edge.management_path_prefix.starts_with('/')
            || self.edge.management_path_prefix.len() > 255
            || self
                .edge
                .management_path_prefix
                .contains(['\0', '\r', '\n', '?', '#'])
            || !valid_env_name(&self.edge.management_auth_token_env)
            || self.edge.domain_verification_timeout_ms == 0
            || self.edge.domain_verification_timeout_ms > 60_000
            || self.edge.certificate_directory.len() > 4096
            || self.edge.certificate_directory.contains(['\0', '\r', '\n'])
            || !certificate_directory.is_absolute()
            || certificate_directory
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            || !(3_600_000..=34_300_800_000).contains(&self.edge.certificate_ttl_ms)
            || self.edge.certificate_renewal_window_ms == 0
            || self.edge.certificate_renewal_window_ms >= self.edge.certificate_ttl_ms
            || self.edge.certificate_reconciliation_interval_ms == 0
            || self.edge.certificate_reconciliation_interval_ms
                > self.edge.certificate_renewal_window_ms
            || self.edge.upstream_request_timeout_ms == 0
            || self.edge.upstream_request_timeout_ms > 3_600_000
            || self.edge.command_ttl_ms == 0
            || self.edge.command_ttl_ms > 86_400_000
        {
            return Err(ConfigError::Invalid(
                "edge requires valid traffic and loopback management addresses, a safe management path/token environment, bounded DNS verification, a normalized certificate directory with bounded lifecycle windows, and independent bounded upstream and command timeouts"
                    .into(),
            ));
        }
        if self.fleet.heartbeat_interval_ms == 0
            || self.fleet.heartbeat_timeout_ms <= self.fleet.heartbeat_interval_ms
            || self.fleet.command_long_poll_ms == 0
            || self.fleet.command_long_poll_ms > 60_000
            || self.fleet.command_lease_ms <= self.fleet.command_long_poll_ms
            || !(300_000..=86_400_000).contains(&self.fleet.certificate_ttl_ms)
            || self.fleet.certificate_rotation_window_ms == 0
            || self.fleet.certificate_rotation_window_ms >= self.fleet.certificate_ttl_ms
        {
            return Err(ConfigError::Invalid(
                "fleet timing requires bounded independent heartbeat, command lease, and certificate windows"
                    .into(),
            ));
        }
        if self.security.state_dir.trim().is_empty()
            || self.security.state_dir.len() > 4096
            || self.security.state_dir.contains('\0')
        {
            return Err(ConfigError::Invalid("security.state_dir is invalid".into()));
        }
        for (label, value) in [
            ("vault_address_env", &self.security.vault_address_env),
            ("vault_token_env", &self.security.vault_token_env),
        ] {
            if !valid_env_name(value) {
                return Err(ConfigError::Invalid(format!(
                    "security.{label} must be an uppercase environment variable name"
                )));
            }
        }
        for (label, value) in [
            ("vault_pki_mount", &self.security.vault_pki_mount),
            ("vault_pki_role", &self.security.vault_pki_role),
            (
                "vault_gateway_pki_mount",
                &self.security.vault_gateway_pki_mount,
            ),
            (
                "vault_gateway_pki_role",
                &self.security.vault_gateway_pki_role,
            ),
            ("vault_transit_mount", &self.security.vault_transit_mount),
            ("vault_transit_key", &self.security.vault_transit_key),
        ] {
            if !valid_provider_segment(value) {
                return Err(ConfigError::Invalid(format!("security.{label} is invalid")));
            }
        }
        if self.security.vault_timeout_ms == 0 || self.security.vault_timeout_ms > 60_000 {
            return Err(ConfigError::Invalid(
                "security.vault_timeout_ms must be between 1 and 60000".into(),
            ));
        }
        if self.security.profile == SecurityProfile::Production
            && (self.security.certificate_authority != SecurityProviderKind::Vault
                || self.security.gateway_certificate_authority != SecurityProviderKind::Vault
                || self.security.key_encryption != SecurityProviderKind::Vault)
        {
            return Err(ConfigError::Invalid(
                "production security requires external Vault node PKI, Gateway PKI, and Transit providers"
                    .into(),
            ));
        }
        if self.security.profile == SecurityProfile::Production
            && (self.logs.storage_provider != LogStorageProviderKind::S3 || self.logs.s3_allow_http)
        {
            return Err(ConfigError::Invalid(
                "production security requires HTTPS S3-compatible log storage".into(),
            ));
        }
        Ok(())
    }

    fn validate_github_app(&self) -> Result<(), ConfigError> {
        let sources = &self.sources;
        if !sources.github_app_enabled {
            if [
                &sources.github_app_slug,
                &sources.github_app_client_id,
                &sources.github_app_client_secret_env,
                &sources.github_app_private_key_env,
                &sources.github_app_callback_url,
            ]
            .into_iter()
            .any(|value| !value.is_empty())
            {
                return Err(ConfigError::Invalid(
                    "disabled sources GitHub App fields must be empty".into(),
                ));
            }
            return Ok(());
        }
        if !valid_github_app_slug(&sources.github_app_slug) {
            return Err(ConfigError::Invalid(
                "sources.github_app_slug must use bounded lowercase GitHub App slug syntax".into(),
            ));
        }
        if !valid_github_client_id(&sources.github_app_client_id) {
            return Err(ConfigError::Invalid(
                "sources.github_app_client_id is invalid".into(),
            ));
        }
        if !valid_env_name(&sources.github_app_client_secret_env) {
            return Err(ConfigError::Invalid(
                "sources.github_app_client_secret_env must be an uppercase environment variable name"
                    .into(),
            ));
        }
        if !valid_env_name(&sources.github_app_private_key_env) {
            return Err(ConfigError::Invalid(
                "sources.github_app_private_key_env must be an uppercase environment variable name"
                    .into(),
            ));
        }
        if !valid_github_callback_url(&sources.github_app_callback_url) {
            return Err(ConfigError::Invalid(
                "sources.github_app_callback_url must be an HTTPS URL ending at /api/v1/source-connections/github/callback"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn server_address(&self) -> Result<SocketAddr, ConfigError> {
        format!("{}:{}", self.server.host, self.server.port)
            .parse()
            .map_err(|error| ConfigError::Invalid(format!("invalid server address: {error}")))
    }

    pub fn node_control_address(&self) -> Result<SocketAddr, ConfigError> {
        format!("{}:{}", self.node_control.host, self.node_control.port)
            .parse()
            .map_err(|error| ConfigError::Invalid(format!("invalid node-control address: {error}")))
    }

    pub fn postgres_url(&self) -> Result<String, ConfigError> {
        std::env::var(&self.postgres.url_env).map_err(|_| {
            ConfigError::Invalid(format!(
                "required environment variable {:?} is not set",
                self.postgres.url_env
            ))
        })
    }

    pub fn nats_url(&self) -> Result<Option<String>, ConfigError> {
        if self.events.provider == EventProviderKind::Memory {
            return Ok(None);
        }
        std::env::var(&self.events.nats_url_env)
            .map(Some)
            .map_err(|_| {
                ConfigError::Invalid(format!(
                    "required environment variable {:?} is not set",
                    self.events.nats_url_env
                ))
            })
    }

    pub fn bootstrap_token(&self) -> Result<String, ConfigError> {
        let value = std::env::var(&self.auth.bootstrap_token_env).map_err(|_| {
            ConfigError::Invalid(format!(
                "required environment variable {:?} is not set",
                self.auth.bootstrap_token_env
            ))
        })?;
        if value.len() < 32 || value.len() > 512 || value.contains(['\0', '\r', '\n']) {
            return Err(ConfigError::Invalid(format!(
                "environment variable {:?} must contain 32 to 512 safe bytes",
                self.auth.bootstrap_token_env
            )));
        }
        Ok(value)
    }

    pub fn vault_credentials(&self) -> Result<Option<(String, String)>, ConfigError> {
        if self.security.certificate_authority != SecurityProviderKind::Vault
            && self.security.gateway_certificate_authority != SecurityProviderKind::Vault
            && self.security.key_encryption != SecurityProviderKind::Vault
        {
            return Ok(None);
        }
        let address = required_environment(&self.security.vault_address_env)?;
        let token = required_environment(&self.security.vault_token_env)?;
        if token.is_empty() || token.len() > 8192 || token.contains(['\0', '\r', '\n']) {
            return Err(ConfigError::Invalid(format!(
                "environment variable {:?} is not a valid Vault token",
                self.security.vault_token_env
            )));
        }
        Ok(Some((address, token)))
    }

    pub(crate) fn s3_log_credentials(&self) -> Result<Option<S3LogCredentials>, ConfigError> {
        if self.logs.storage_provider == LogStorageProviderKind::Local {
            return Ok(None);
        }
        let access_key_id = required_environment(&self.logs.s3_access_key_env)?;
        let secret_access_key = required_environment(&self.logs.s3_secret_key_env)?;
        let session_token = if self.logs.s3_session_token_env.is_empty() {
            None
        } else {
            Some(required_environment(&self.logs.s3_session_token_env)?)
        };
        if !valid_credential(&access_key_id, 1024)
            || !valid_credential(&secret_access_key, 8192)
            || session_token
                .as_deref()
                .is_some_and(|token| !valid_credential(token, 8192))
        {
            return Err(ConfigError::Invalid(
                "S3 credential environment variables contain invalid values".into(),
            ));
        }
        Ok(Some(S3LogCredentials {
            access_key_id,
            secret_access_key,
            session_token,
        }))
    }
}

pub(crate) struct S3LogCredentials {
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) session_token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read Cloud config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid Cloud config: {0}")]
    Invalid(String),
}

fn validate_root(document: &Document) -> Result<(), ConfigError> {
    let allowed = [
        "artifacts",
        "auth",
        "events",
        "deployments",
        "edge",
        "fleet",
        "logs",
        "node_control",
        "operations",
        "postgres",
        "registry",
        "security",
        "server",
        "sources",
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

fn validate_unique_repositories(label: &str, values: &[String]) -> Result<(), ConfigError> {
    let identities = values
        .iter()
        .map(|value| {
            GitRepository::parse(GitProvider::Github, value)
                .map(|repository| repository.identity().to_owned())
                .map_err(|error| ConfigError::Invalid(format!("{label} is invalid: {error}")))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if identities.len() != values.len() {
        return Err(ConfigError::Invalid(format!(
            "{label} cannot contain canonical duplicates"
        )));
    }
    Ok(())
}

fn valid_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_github_app_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_github_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_github_callback_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/api/v1/source-connections/github/callback"
}

fn valid_provider_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_s3_region(value: &str) -> bool {
    value.len() <= 64 && valid_provider_segment(value)
}

fn valid_s3_bucket(value: &str) -> bool {
    (3..=63).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn valid_object_prefix(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && !matches!(segment, "." | "..")
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

fn valid_s3_endpoint(value: &str, allow_http: bool) -> bool {
    if value.is_empty() {
        return !allow_http;
    }
    let Ok(endpoint) = Url::parse(value) else {
        return false;
    };
    (endpoint.scheme() == "https" || allow_http && endpoint.scheme() == "http")
        && endpoint.host_str().is_some()
        && endpoint.username().is_empty()
        && endpoint.password().is_none()
        && endpoint.query().is_none()
        && endpoint.fragment().is_none()
        && matches!(endpoint.path(), "" | "/")
}

fn valid_credential(value: &str, max_len: usize) -> bool {
    !value.is_empty() && value.len() <= max_len && !value.contains(['\0', '\r', '\n'])
}

fn required_environment(name: &str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| {
        ConfigError::Invalid(format!("required environment variable {name:?} is not set"))
    })
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

fn boolean(block: &Block, field: &str) -> Result<bool, ConfigError> {
    block
        .attributes
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| ConfigError::Invalid(format!("{}.{} must be a boolean", block.name, field)))
}

fn string_list(block: &Block, field: &str) -> Result<Vec<String>, ConfigError> {
    let values = match block.attributes.get(field) {
        Some(Value::List(values)) => values,
        _ => {
            return Err(ConfigError::Invalid(format!(
                "{}.{} must be a list of strings",
                block.name, field
            )))
        }
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "{}.{} must contain only strings",
                    block.name, field
                ))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
server { host = "127.0.0.1" port = 8080 role = "all" }
node_control {
  host = "127.0.0.1"
  port = 8443
  server_name = "localhost"
  certificate_file = ".a3s/cloud/security/node-control/server.pem"
  private_key_file = ".a3s/cloud/security/node-control/server-key.pem"
  client_ca_file = ".a3s/cloud/security/node-ca/ca.pem"
  max_request_bytes = 20971520
  tls_handshake_timeout_ms = 5000
  request_body_timeout_ms = 10000
}
artifacts {
  store_dir = ".a3s/cloud/artifacts"
  max_blob_bytes = 1073741824
  transfer_timeout_ms = 900000
}
postgres { url_env = "A3S_CLOUD_POSTGRES_URL" max_connections = 16 }
auth { bootstrap_token_env = "A3S_CLOUD_BOOTSTRAP_TOKEN" }
events {
  provider = "memory"
  nats_url_env = "A3S_CLOUD_NATS_URL"
  stream_name = "A3S_CLOUD_EVENTS"
  batch_size = 100
  poll_interval_ms = 250
  lease_ms = 10000
  publish_timeout_ms = 3000
  retry_initial_ms = 500
  retry_max_ms = 30000
}
operations { reconcile_interval_ms = 5000 lease_ms = 30000 }
deployments {
  reconcile_interval_ms = 30000
  command_ttl_ms = 180000
  runtime_apply_timeout_ms = 120000
  observation_poll_ms = 1000
  convergence_timeout_ms = 600000
  runtime_stop_timeout_ms = 60000
  cleanup_poll_ms = 1000
  cleanup_timeout_ms = 300000
}
registry {
  request_timeout_ms = 10000
  insecure_hosts = ["127.0.0.1:5000"]
}
sources {
  github_request_timeout_ms = 10000
  github_webhook_secret_env = "A3S_CLOUD_GITHUB_WEBHOOK_SECRET"
  github_webhook_max_body_bytes = 1048576
  github_app_enabled = true
  github_app_slug = "a3s-cloud-test"
  github_app_client_id = "Iv1.test-client"
  github_app_client_secret_env = "A3S_CLOUD_GITHUB_APP_CLIENT_SECRET"
  github_app_private_key_env = "A3S_CLOUD_GITHUB_APP_PRIVATE_KEY"
  github_app_callback_url = "https://cloud.example.test/api/v1/source-connections/github/callback"
  github_connection_state_ttl_ms = 600000
  allowed_repositories = ["https://github.com/A3S-Lab/Cloud"]
  denied_repositories = []
}
logs {
  storage_provider = "local"
  s3_endpoint = ""
  s3_region = "us-east-1"
  s3_bucket = "a3s-cloud-logs"
  s3_prefix = "logs"
  s3_access_key_env = "A3S_CLOUD_S3_ACCESS_KEY_ID"
  s3_secret_key_env = "A3S_CLOUD_S3_SECRET_ACCESS_KEY"
  s3_session_token_env = ""
  s3_allow_http = false
  s3_virtual_hosted_style = false
  s3_request_timeout_ms = 30000
  s3_connect_timeout_ms = 5000
  s3_retry_timeout_ms = 60000
  s3_max_retries = 3
  retention_ms = 604800000
  retention_poll_ms = 60000
  retention_batch_size = 256
  tombstone_retention_ms = 2592000000
  tombstone_compaction_poll_ms = 3600000
  tombstone_compaction_batch_size = 1000
}
edge {
  entrypoint_address = "0.0.0.0:8081"
  management_address = "127.0.0.1:9090"
  management_path_prefix = "/api/gateway"
  management_auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  domain_verification_timeout_ms = 5000
  certificate_directory = "/var/lib/a3s-cloud/gateway/certificates"
  certificate_ttl_ms = 2592000000
  certificate_renewal_window_ms = 604800000
  certificate_reconciliation_interval_ms = 60000
  upstream_request_timeout_ms = 30000
  command_ttl_ms = 180000
}
fleet {
  heartbeat_interval_ms = 5000
  heartbeat_timeout_ms = 20000
  command_long_poll_ms = 25000
  command_lease_ms = 30000
  certificate_ttl_ms = 3600000
  certificate_rotation_window_ms = 900000
}
security {
  profile = "development"
  state_dir = ".a3s/cloud/security"
  certificate_authority = "local"
  gateway_certificate_authority = "local"
  key_encryption = "local"
  vault_address_env = "A3S_CLOUD_VAULT_ADDR"
  vault_token_env = "A3S_CLOUD_VAULT_TOKEN"
  vault_pki_mount = "pki"
  vault_pki_role = "a3s-cloud-node"
  vault_gateway_pki_mount = "gateway-pki"
  vault_gateway_pki_role = "a3s-cloud-gateway"
  vault_transit_mount = "transit"
  vault_transit_key = "a3s-cloud"
  vault_timeout_ms = 5000
}
"#;

    #[test]
    fn parses_closed_acl_configuration() {
        let config = CloudConfig::parse(VALID).expect("valid config");
        assert_eq!(config.server.role, ProcessRole::All);
        assert_eq!(config.server_address().expect("address").port(), 8080);
        assert_eq!(config.postgres.max_connections, 16);
        assert_eq!(config.auth.bootstrap_token_env, "A3S_CLOUD_BOOTSTRAP_TOKEN");
        assert_eq!(config.events.provider, EventProviderKind::Memory);
        assert_eq!(config.sources.allowed_repositories.len(), 1);
        assert_eq!(
            config.sources.github_webhook_secret_env,
            "A3S_CLOUD_GITHUB_WEBHOOK_SECRET"
        );
        assert_eq!(config.sources.github_webhook_max_body_bytes, 1_048_576);
        assert!(config.sources.github_app_enabled);
        assert_eq!(config.sources.github_app_slug, "a3s-cloud-test");
        assert_eq!(
            config.sources.github_app_private_key_env,
            "A3S_CLOUD_GITHUB_APP_PRIVATE_KEY"
        );
        assert_eq!(
            config.sources.github_app_callback_url,
            "https://cloud.example.test/api/v1/source-connections/github/callback"
        );
        assert_eq!(config.sources.github_connection_state_ttl_ms, 600_000);
        assert_eq!(config.logs.storage_provider, LogStorageProviderKind::Local);
        assert_eq!(config.logs.retention_batch_size, 256);
        assert_eq!(config.logs.tombstone_compaction_batch_size, 1000);
        assert_eq!(config.edge.domain_verification_timeout_ms, 5_000);
        assert_eq!(config.security.profile, SecurityProfile::Development);
        assert_eq!(
            config.security.gateway_certificate_authority,
            SecurityProviderKind::Local
        );
    }

    #[test]
    fn loads_shipped_cloud_acl() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/cloud.acl");
        let config = CloudConfig::load(&path)
            .unwrap_or_else(|error| panic!("failed to load {}: {error}", path.display()));

        assert_eq!(config.server.role, ProcessRole::All);
        assert_eq!(
            config.server_address().expect("server address").port(),
            8080
        );
        assert_eq!(config.events.provider, EventProviderKind::Memory);
        assert_eq!(config.sources.github_request_timeout_ms, 10_000);
        assert_eq!(config.sources.github_webhook_max_body_bytes, 1_048_576);
        assert!(!config.sources.github_app_enabled);
        assert!(config.sources.github_app_slug.is_empty());
        assert!(config.sources.github_app_private_key_env.is_empty());
        assert_eq!(config.logs.retention_ms, 604_800_000);
        assert_eq!(config.security.profile, SecurityProfile::Development);
    }

    #[test]
    fn rejects_unknown_fields_and_unsafe_timing() {
        assert!(CloudConfig::parse(
            &VALID.replace("role = \"all\"", "role = \"all\" debug = true")
        )
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace("lease_ms = 30000", "lease_ms = 1000")).is_err());
        assert!(CloudConfig::parse(
            &VALID.replace("publish_timeout_ms = 3000", "publish_timeout_ms = 10000")
        )
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "domain_verification_timeout_ms = 5000",
            "domain_verification_timeout_ms = 0"
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "domain_verification_timeout_ms = 5000",
            "domain_verification_timeout_ms = 60001"
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "certificate_reconciliation_interval_ms = 60000",
            "certificate_reconciliation_interval_ms = 604800001"
        ))
        .is_err());
        assert!(CloudConfig::parse(
            &VALID.replace("profile = \"development\"", "profile = \"production\"")
        )
        .is_err());
        assert!(CloudConfig::parse(
            &VALID.replace("retention_poll_ms = 60000", "retention_poll_ms = 604800001")
        )
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "tombstone_compaction_poll_ms = 3600000",
            "tombstone_compaction_poll_ms = 2592000001"
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "s3_endpoint = \"\"",
            "s3_endpoint = \"http://object-store\""
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "s3_bucket = \"a3s-cloud-logs\"",
            "s3_bucket = \"Invalid_Bucket\""
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "allowed_repositories = [\"https://github.com/A3S-Lab/Cloud\"]",
            "allowed_repositories = []"
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "allowed_repositories = [\"https://github.com/A3S-Lab/Cloud\"]",
            "allowed_repositories = [\"https://github.com.evil.example/A3S-Lab/Cloud\"]"
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "github_webhook_secret_env = \"A3S_CLOUD_GITHUB_WEBHOOK_SECRET\"",
            "github_webhook_secret_env = \"webhook-secret\""
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "github_webhook_max_body_bytes = 1048576",
            "github_webhook_max_body_bytes = 1023"
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "github_app_slug = \"a3s-cloud-test\"",
            "github_app_slug = \"A3S Cloud\""
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "github_app_private_key_env = \"A3S_CLOUD_GITHUB_APP_PRIVATE_KEY\"",
            "github_app_private_key_env = \"github-private-key\""
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "https://cloud.example.test/api/v1/source-connections/github/callback",
            "http://cloud.example.test/api/v1/source-connections/github/callback"
        ))
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "github_connection_state_ttl_ms = 600000",
            "github_connection_state_ttl_ms = 59999"
        ))
        .is_err());
        assert!(CloudConfig::parse(
            &VALID.replace("github_app_enabled = true", "github_app_enabled = false")
        )
        .is_err());
        let disabled_github_app = VALID
            .replace("github_app_enabled = true", "github_app_enabled = false")
            .replace("github_app_slug = \"a3s-cloud-test\"", "github_app_slug = \"\"")
            .replace(
                "github_app_client_id = \"Iv1.test-client\"",
                "github_app_client_id = \"\"",
            )
            .replace(
                "github_app_client_secret_env = \"A3S_CLOUD_GITHUB_APP_CLIENT_SECRET\"",
                "github_app_client_secret_env = \"\"",
            )
            .replace(
                "github_app_private_key_env = \"A3S_CLOUD_GITHUB_APP_PRIVATE_KEY\"",
                "github_app_private_key_env = \"\"",
            )
            .replace(
                "github_app_callback_url = \"https://cloud.example.test/api/v1/source-connections/github/callback\"",
                "github_app_callback_url = \"\"",
            );
        assert!(CloudConfig::parse(&disabled_github_app).is_ok());

        let development_s3 = VALID
            .replace("storage_provider = \"local\"", "storage_provider = \"s3\"")
            .replace(
                "s3_endpoint = \"\"",
                "s3_endpoint = \"http://127.0.0.1:9000\"",
            )
            .replace("s3_allow_http = false", "s3_allow_http = true");
        assert!(CloudConfig::parse(&development_s3).is_ok());

        let production_s3 = VALID
            .replace("profile = \"development\"", "profile = \"production\"")
            .replace(
                "  gateway_certificate_authority = \"local\"",
                "  gateway_certificate_authority = \"vault\"",
            )
            .replace(
                "  certificate_authority = \"local\"",
                "  certificate_authority = \"vault\"",
            )
            .replace("key_encryption = \"local\"", "key_encryption = \"vault\"")
            .replace("storage_provider = \"local\"", "storage_provider = \"s3\"");
        assert!(CloudConfig::parse(&production_s3).is_ok());
        assert!(CloudConfig::parse(&production_s3.replace(
            "gateway_certificate_authority = \"vault\"",
            "gateway_certificate_authority = \"local\""
        ))
        .is_err());
        assert!(CloudConfig::parse(
            &production_s3
                .replace(
                    "s3_endpoint = \"\"",
                    "s3_endpoint = \"http://127.0.0.1:9000\""
                )
                .replace("s3_allow_http = false", "s3_allow_http = true")
        )
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace(
            "s3_endpoint = \"\"",
            "s3_endpoint = \"https://credential@objects.example\""
        ))
        .is_err());
    }
}
