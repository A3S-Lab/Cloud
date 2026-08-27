use super::*;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

pub(super) const URL_ENV: &str = "A3S_CLOUD_INTEGRATION_POSTGRES_URL";
pub(super) const BOOTSTRAP_ENV: &str = "A3S_CLOUD_INTEGRATION_BOOTSTRAP_TOKEN";
pub(super) const BOOTSTRAP_TOKEN: &str = "integration-bootstrap-credential-0123456789abcdef";
pub(super) const GITHUB_WEBHOOK_ENV: &str = "A3S_CLOUD_INTEGRATION_GITHUB_WEBHOOK_SECRET";
pub(super) const GITHUB_WEBHOOK_SECRET: &str = "integration-github-webhook-secret-0123456789abcdef";
pub(super) const ADMIN_TOKEN: &str =
    "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const PROJECT_TOKEN: &str =
    "a3s_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const EXPIRING_TOKEN: &str =
    "a3s_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(super) const SERVICE_MEMBER_TOKEN: &str =
    "a3s_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
pub(super) const PRIVILEGE_ESCALATION_TOKEN: &str =
    "a3s_0000000000000000000000000000000000000000000000000000000000000000";

pub(super) fn serving_role_for_url(database_url: &str) -> Result<String, PostgresBootstrapError> {
    let parsed = url::Url::parse(database_url).map_err(|error| {
        PostgresBootstrapError::ComponentConfiguration(format!(
            "invalid isolated PostgreSQL URL: {error}"
        ))
    })?;
    let database_name = parsed.path().trim_start_matches('/');
    if database_name.is_empty() || database_name.contains('/') {
        return Err(PostgresBootstrapError::ComponentConfiguration(
            "isolated PostgreSQL URL must name exactly one database".into(),
        ));
    }
    Ok(format!("{database_name}_serving"))
}

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
    serving_role: String,
}

impl IsolatedPostgresDatabase {
    pub(super) async fn create(admin_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let database_name = format!("a3s_cloud_test_{}", Uuid::new_v4().simple());
        let mut database_url = url::Url::parse(admin_url)?;
        database_url.set_path(&format!("/{database_name}"));
        let serving_role = format!("{database_name}_serving");

        let admin = PostgresExecutor::connect_no_tls(admin_url, 2)?;
        let connection = admin.pool().get().await?;
        connection
            .batch_execute(&format!("create database \"{database_name}\""))
            .await?;
        if let Err(source) = connection
            .batch_execute(&format!(
                "create role \"{serving_role}\" nologin nosuperuser nocreatedb nocreaterole noreplication"
            ))
            .await
        {
            let cleanup = connection
                .batch_execute(&format!(
                    "drop database if exists \"{database_name}\" with (force)"
                ))
                .await;
            return match cleanup {
                Ok(()) => Err(source.into()),
                Err(cleanup_error) => Err(std::io::Error::other(format!(
                    "could not create isolated serving role: {source}; database cleanup also failed: {cleanup_error}"
                ))
                .into()),
            };
        }

        Ok(Self {
            admin_url: admin_url.to_owned(),
            database_name,
            database_url: database_url.to_string(),
            serving_role,
        })
    }

    pub(super) fn url(&self) -> &str {
        &self.database_url
    }

    pub(super) async fn cleanup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let admin = PostgresExecutor::connect_no_tls(&self.admin_url, 2)?;
        let connection = admin.pool().get().await?;
        let database_cleanup = connection
            .batch_execute(&format!(
                "drop database if exists \"{}\" with (force)",
                self.database_name
            ))
            .await;
        let role_cleanup = connection
            .batch_execute(&format!("drop role if exists \"{}\"", self.serving_role))
            .await;
        match (database_cleanup, role_cleanup) {
            (Ok(()), Ok(())) => {}
            (Err(database_error), Ok(())) => return Err(database_error.into()),
            (Ok(()), Err(role_error)) => return Err(role_error.into()),
            (Err(database_error), Err(role_error)) => {
                return Err(std::io::Error::other(format!(
                    "isolated database cleanup failed: {database_error}; role cleanup also failed: {role_error}"
                ))
                .into());
            }
        }
        let row = connection
            .query_one(
                "select exists(select 1 from pg_database where datname = $1), exists(select 1 from pg_roles where rolname = $2)",
                &[&self.database_name, &self.serving_role],
            )
            .await?;
        let database_still_exists: bool = row.get(0);
        let role_still_exists: bool = row.get(1);
        if database_still_exists || role_still_exists {
            return Err(std::io::Error::other(format!(
                "isolated PostgreSQL resources still exist after cleanup (database={}, role={})",
                self.database_name, self.serving_role
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

pub(super) fn github_webhook_request(event: &str, delivery_id: &str, body: &[u8]) -> BootRequest {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(GITHUB_WEBHOOK_SECRET.as_bytes()).expect("HMAC");
    mac.update(body);
    BootRequest::new(HttpMethod::Post, "/api/v1/webhooks/github")
        .with_header("content-type", "application/json")
        .with_header("x-github-event", event)
        .with_header("x-github-delivery", delivery_id)
        .with_header(
            "x-hub-signature-256",
            format!("sha256={:x}", mac.finalize().into_bytes()),
        )
        .with_body(body.to_vec())
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
        artifacts: ArtifactTransferConfig {
            max_blob_bytes: 1024 * 1024 * 1024,
            transfer_timeout_ms: 900_000,
        },
        assets: AssetsConfig {
            repository_dir: ".a3s/integration-asset-repositories".into(),
            git_command_timeout_ms: 10_000,
            write_lease_ms: 30_000,
            repository_quota_bytes: 1024 * 1024 * 1024,
            max_rpc_body_bytes: 64 * 1024 * 1024,
            backup_max_bytes: 1024 * 1024 * 1024,
        },
        objects: ObjectStorageConfig {
            provider: ObjectStorageProviderKind::Local,
            local_dir: ".a3s/integration-objects".into(),
            endpoint: String::new(),
            region: "us-east-1".into(),
            bucket: "a3s-cloud-objects".into(),
            prefix: "cloud".into(),
            access_key_env: "A3S_CLOUD_S3_ACCESS_KEY_ID".into(),
            secret_key_env: "A3S_CLOUD_S3_SECRET_ACCESS_KEY".into(),
            session_token_env: String::new(),
            allow_http: false,
            virtual_hosted_style: false,
            request_timeout_ms: 30_000,
            connect_timeout_ms: 5_000,
            retry_timeout_ms: 60_000,
            max_retries: 3,
        },
        postgres: PostgresConfig {
            serving_role: "a3s_cloud_serving".into(),
            serving_url_env: URL_ENV.into(),
            migration_url_env: "A3S_CLOUD_INTEGRATION_POSTGRES_MIGRATION_URL".into(),
            max_connections: 8,
        },
        auth: AuthConfig {
            bootstrap_token_env: BOOTSTRAP_ENV.into(),
            oidc_providers: Vec::new(),
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
        smtp: SmtpConfig {
            provider: SmtpProviderKind::Disabled,
            host: "smtp.example.test".into(),
            port: 465,
            tls: SmtpTlsMode::Implicit,
            hello_name: "cloud.example.test".into(),
            ca_certificate_file: String::new(),
            username_env: "A3S_CLOUD_SMTP_USERNAME".into(),
            password_env: "A3S_CLOUD_SMTP_PASSWORD".into(),
            sender: "no-reply@example.test".into(),
            connect_timeout_ms: 5_000,
            command_timeout_ms: 10_000,
            reservation_lease_ms: 60_000,
        },
        operations: OperationsConfig {
            reconcile_interval_ms: 1_000,
            lease_ms: 5_000,
        },
        human_tasks: HumanTasksConfig {
            coordination_poll_interval_ms: 100,
            coordination_batch_size: 100,
            resume_poll_interval_ms: 100,
            resume_batch_size: 100,
            resume_lease_ms: 5_000,
            flow_operation_timeout_ms: 1_000,
            retry_initial_ms: 100,
            retry_max_ms: 5_000,
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
        executions: a3s_cloud_control_plane::config::ExecutionsConfig {
            reconcile_interval_ms: 1_000,
            command_ttl_ms: 900_000,
            observation_poll_ms: 10,
            convergence_timeout_ms: 20_000,
            cleanup_timeout_ms: 20_000,
            checkpoint_object_reconcile_interval_ms: 1_000,
            checkpoint_object_capture_lease_ms: 120_000,
            checkpoint_object_orphan_grace_ms: 600_000,
            checkpoint_object_cleanup_lease_ms: 120_000,
            checkpoint_object_reconcile_batch_size: 100,
        },
        builds: BuildsConfig {
            reconcile_interval_ms: 1_000,
            input_staging_dir: ".a3s/integration-build-input".into(),
            input_max_entries: 10_000,
            input_max_bytes: 128 * 1024 * 1024,
            output_staging_dir: ".a3s/integration-build-output".into(),
            output_max_entries: 10_000,
            output_max_expanded_bytes: 256 * 1024 * 1024,
            oci_max_blobs: 1_000,
            oci_max_bytes: 256 * 1024 * 1024,
            command_ttl_ms: 10_000,
            execution_timeout_ms: 5_000,
            observation_poll_ms: 10,
            convergence_timeout_ms: 20_000,
            cleanup_timeout_ms: 20_000,
            output_max_bytes: 128 * 1024 * 1024,
            cache_max_bytes: 128 * 1024 * 1024,
        },
        registry: RegistryConfig {
            request_timeout_ms: 10_000,
            insecure_hosts: vec!["127.0.0.1:5000".into()],
            publication_registry: "127.0.0.1:5000".into(),
            publication_repository_prefix: "a3s-cloud/builds".into(),
            publication_credential_env: String::new(),
            publication_allow_anonymous: true,
            publication_timeout_ms: 60_000,
        },
        sources: SourcesConfig {
            github_request_timeout_ms: 10_000,
            github_webhook_secret_env: "A3S_CLOUD_INTEGRATION_GITHUB_WEBHOOK_SECRET".into(),
            github_webhook_max_body_bytes: 1024 * 1024,
            github_app_enabled: false,
            github_app_slug: String::new(),
            github_app_client_id: String::new(),
            github_app_client_secret_env: String::new(),
            github_app_private_key_env: String::new(),
            github_app_callback_url: String::new(),
            github_connection_state_ttl_ms: 600_000,
            github_authority_reconcile_interval_ms: 10_000,
            github_authority_poll_interval_ms: 300_000,
            github_authority_retry_initial_ms: 1_000,
            github_authority_retry_max_ms: 60_000,
            github_authority_batch_size: 100,
            checkout_dir: ".a3s/integration-source-checkouts".into(),
            checkout_timeout_ms: 10_000,
            checkout_max_files: 10_000,
            checkout_max_bytes: 64 * 1024 * 1024,
            allowed_repositories: vec!["https://github.com/A3S-Lab/Cloud".into()],
            denied_repositories: Vec::new(),
        },
        logs: LogsConfig {
            retention_ms: 60_000,
            retention_poll_ms: 1_000,
            retention_batch_size: 16,
            tombstone_retention_ms: 300_000,
            tombstone_compaction_poll_ms: 10_000,
            tombstone_compaction_batch_size: 64,
        },
        audit: AuditConfig {
            retention_ms: 7_776_000_000,
            retention_poll_ms: 60_000,
            retention_organization_batch_size: 32,
            retention_record_batch_size: 256,
        },
        edge: EdgeConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            domain_verification_timeout_ms: 5_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
            certificate_ttl_ms: 2_592_000_000,
            certificate_renewal_window_ms: 604_800_000,
            snapshot_renewal_window_ms: 21_600_000,
            certificate_reconciliation_interval_ms: 60_000,
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
            gateway_certificate_authority: SecurityProviderKind::Local,
            key_encryption: SecurityProviderKind::Local,
            build_evidence_signing: SecurityProviderKind::Local,
            audit_export_signing: SecurityProviderKind::Local,
            recipient_contact_proof: SecurityProviderKind::Local,
            recipient_contact_proof_key_id: "recipient-contact-v1".into(),
            vault_address_env: "A3S_CLOUD_VAULT_ADDR".into(),
            vault_token_env: "A3S_CLOUD_VAULT_TOKEN".into(),
            vault_pki_mount: "pki".into(),
            vault_pki_role: "a3s-cloud-node".into(),
            vault_gateway_pki_mount: "gateway-pki".into(),
            vault_gateway_pki_role: "a3s-cloud-gateway".into(),
            vault_transit_mount: "transit".into(),
            vault_transit_key: "a3s-cloud".into(),
            vault_build_evidence_signing_key: "a3s-cloud-build-evidence".into(),
            vault_audit_export_signing_key: "a3s-cloud-audit-export".into(),
            vault_recipient_contact_proof_key: "a3s-cloud-recipient-contact-proof".into(),
            vault_timeout_ms: 5_000,
        },
    }
}

pub(super) fn configure_ephemeral_application_state(
    config: &mut CloudConfig,
    root: &Path,
) -> PathBuf {
    config.security.state_dir = root.display().to_string();
    config.node_control.certificate_file =
        root.join("node-control/server.pem").display().to_string();
    config.node_control.private_key_file = root
        .join("node-control/server-key.pem")
        .display()
        .to_string();
    config.node_control.client_ca_file = root.join("node-ca/ca.pem").display().to_string();
    let asset_repository_directory = root.join("asset-repositories");
    config.assets.repository_dir = asset_repository_directory.display().to_string();
    config.objects.local_dir = root.join("immutable-objects").display().to_string();
    asset_repository_directory
}
