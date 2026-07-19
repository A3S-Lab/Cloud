use super::*;
use std::ffi::{OsStr, OsString};

pub(super) const URL_ENV: &str = "A3S_CLOUD_INTEGRATION_POSTGRES_URL";
pub(super) const BOOTSTRAP_ENV: &str = "A3S_CLOUD_INTEGRATION_BOOTSTRAP_TOKEN";
pub(super) const BOOTSTRAP_TOKEN: &str = "integration-bootstrap-credential-0123456789abcdef";
pub(super) const ADMIN_TOKEN: &str =
    "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const PROJECT_TOKEN: &str =
    "a3s_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const EXPIRING_TOKEN: &str =
    "a3s_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

pub(super) struct EnvironmentOverride {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvironmentOverride {
    pub(super) fn set(name: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvironmentOverride {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

pub(super) struct IsolatedPostgresDatabase {
    admin_url: String,
    database_name: String,
    database_url: String,
}

impl IsolatedPostgresDatabase {
    pub(super) async fn create(admin_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let database_name = format!("a3s_cloud_test_{}", Uuid::new_v4().simple());
        let mut database_url = url::Url::parse(admin_url)?;
        database_url.set_path(&format!("/{database_name}"));

        let admin = PostgresExecutor::connect_no_tls(admin_url, 2)?;
        admin
            .pool()
            .get()
            .await?
            .batch_execute(&format!("create database \"{database_name}\""))
            .await?;

        Ok(Self {
            admin_url: admin_url.to_owned(),
            database_name,
            database_url: database_url.to_string(),
        })
    }

    pub(super) fn url(&self) -> &str {
        &self.database_url
    }

    pub(super) async fn cleanup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let admin = PostgresExecutor::connect_no_tls(&self.admin_url, 2)?;
        let connection = admin.pool().get().await?;
        connection
            .batch_execute(&format!(
                "drop database if exists \"{}\" with (force)",
                self.database_name
            ))
            .await?;
        let row = connection
            .query_one(
                "select exists(select 1 from pg_database where datname = $1)",
                &[&self.database_name],
            )
            .await?;
        let database_still_exists: bool = row.get(0);
        if database_still_exists {
            return Err(std::io::Error::other(format!(
                "isolated PostgreSQL database {} still exists after cleanup",
                self.database_name
            ))
            .into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CompletingRuntime;

#[async_trait]
impl FlowRuntime for CompletingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let output = invocation.input.clone();
        Ok(invocation.context().complete(output))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(format!(
            "integration runtime does not support step {:?}",
            invocation.step_name
        )))
    }
}

pub(super) fn post_json(
    path: impl Into<String>,
    idempotency_key: &str,
    body: Value,
) -> BootRequest {
    post_json_as(path, idempotency_key, body, ADMIN_TOKEN)
}

pub(super) fn post_json_as(
    path: impl Into<String>,
    idempotency_key: &str,
    body: Value,
    token: &str,
) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path.into())
        .with_header("content-type", "application/json")
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
        .with_body(body.to_string().into_bytes())
}

pub(super) fn delete_as(
    path: impl Into<String>,
    idempotency_key: &str,
    token: &str,
) -> BootRequest {
    BootRequest::new(HttpMethod::Delete, path.into())
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
}

pub(super) fn get_as(path: impl Into<String>, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Get, path.into())
        .with_header("accept", "application/json")
        .with_header("authorization", format!("Bearer {token}"))
}

pub(super) fn response_json(response: &BootResponse) -> a3s_boot::Result<Value> {
    response.body_json()
}

pub(super) fn response_id(response: &BootResponse) -> a3s_boot::Result<String> {
    response_json(response)?["data"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal("response does not contain a resource ID".into()))
}

pub(super) fn config() -> CloudConfig {
    CloudConfig {
        server: ServerConfig {
            host: "127.0.0.1".into(),
            port: 8080,
            role: ProcessRole::All,
        },
        node_control: NodeControlConfig {
            host: "127.0.0.1".into(),
            port: 8443,
            server_name: "localhost".into(),
            certificate_file: ".a3s/integration-security/node-control/server.pem".into(),
            private_key_file: ".a3s/integration-security/node-control/server-key.pem".into(),
            client_ca_file: ".a3s/integration-security/node-ca/ca.pem".into(),
            max_request_bytes: 20 * 1024 * 1024,
            tls_handshake_timeout_ms: 5_000,
            request_body_timeout_ms: 10_000,
        },
        postgres: PostgresConfig {
            url_env: URL_ENV.into(),
            max_connections: 8,
        },
        auth: AuthConfig {
            bootstrap_token_env: BOOTSTRAP_ENV.into(),
        },
        events: EventsConfig {
            provider: EventProviderKind::Memory,
            nats_url_env: "A3S_CLOUD_NATS_URL".into(),
            stream_name: "A3S_CLOUD_EVENTS".into(),
            batch_size: 100,
            poll_interval_ms: 250,
            lease_ms: 10_000,
            publish_timeout_ms: 3_000,
            retry_initial_ms: 500,
            retry_max_ms: 30_000,
        },
        operations: OperationsConfig {
            reconcile_interval_ms: 1_000,
            lease_ms: 5_000,
        },
        deployments: DeploymentsConfig {
            reconcile_interval_ms: 1_000,
            command_ttl_ms: 10_000,
            runtime_apply_timeout_ms: 5_000,
            observation_poll_ms: 10,
            convergence_timeout_ms: 20_000,
            runtime_stop_timeout_ms: 5_000,
            cleanup_poll_ms: 10,
            cleanup_timeout_ms: 20_000,
        },
        registry: RegistryConfig {
            request_timeout_ms: 10_000,
            insecure_hosts: vec!["127.0.0.1:5000".into()],
        },
        logs: LogsConfig {
            storage_provider: a3s_cloud_control_plane::config::LogStorageProviderKind::Local,
            s3_endpoint: String::new(),
            s3_region: "us-east-1".into(),
            s3_bucket: "a3s-cloud-logs".into(),
            s3_prefix: "logs".into(),
            s3_access_key_env: "A3S_CLOUD_S3_ACCESS_KEY_ID".into(),
            s3_secret_key_env: "A3S_CLOUD_S3_SECRET_ACCESS_KEY".into(),
            s3_session_token_env: String::new(),
            s3_allow_http: false,
            s3_virtual_hosted_style: false,
            s3_request_timeout_ms: 30_000,
            s3_connect_timeout_ms: 5_000,
            s3_retry_timeout_ms: 60_000,
            s3_max_retries: 3,
            retention_ms: 60_000,
            retention_poll_ms: 1_000,
            retention_batch_size: 16,
            tombstone_retention_ms: 300_000,
            tombstone_compaction_poll_ms: 10_000,
            tombstone_compaction_batch_size: 64,
        },
        edge: EdgeConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            certificate_ttl_ms: 2_592_000_000,
            certificate_renewal_window_ms: 604_800_000,
            upstream_request_timeout_ms: 30_000,
            command_ttl_ms: 10_000,
        },
        fleet: FleetConfig {
            heartbeat_interval_ms: 1_000,
            heartbeat_timeout_ms: 5_000,
            command_long_poll_ms: 1_000,
            command_lease_ms: 5_000,
            certificate_ttl_ms: 3_600_000,
            certificate_rotation_window_ms: 900_000,
        },
        security: SecurityConfig {
            profile: SecurityProfile::Development,
            state_dir: ".a3s/integration-security".into(),
            certificate_authority: SecurityProviderKind::Local,
            key_encryption: SecurityProviderKind::Local,
            vault_address_env: "A3S_CLOUD_VAULT_ADDR".into(),
            vault_token_env: "A3S_CLOUD_VAULT_TOKEN".into(),
            vault_pki_mount: "pki".into(),
            vault_pki_role: "a3s-cloud-node".into(),
            vault_transit_mount: "transit".into(),
            vault_transit_key: "a3s-cloud".into(),
            vault_timeout_ms: 5_000,
        },
    }
}
