use super::*;
use crate::config::{
    AuthConfig, DeploymentsConfig, EventProviderKind, EventsConfig, FleetConfig, NodeControlConfig,
    OperationsConfig, PostgresConfig, ProcessRole, RegistryConfig, SecurityConfig, SecurityProfile,
    SecurityProviderKind, ServerConfig,
};
use crate::modules::fleet::domain::entities::{NodeCertificate, NodeCertificateMaterial};
use crate::modules::fleet::domain::services::{CertificateAuthorityError, NodeCertificateRequest};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::InMemoryIdentityRepository;
use crate::modules::operations::InMemoryOperationRepository;
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::workloads::InMemoryWorkloadRepository;
use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod};
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const BOOTSTRAP_TOKEN: &str = "test-bootstrap-credential-0123456789abcdef";
const ADMIN_TOKEN: &str = "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROJECT_TOKEN: &str = "a3s_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EXPIRING_TOKEN: &str = "a3s_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

struct TestCertificateAuthority;

struct TestLogChunkStore;

#[async_trait::async_trait]
impl crate::modules::fleet::domain::services::ILogChunkStore for TestLogChunkStore {
    async fn put(
        &self,
        _batch_id: Uuid,
        _node_id: Uuid,
        ordinal: u16,
        _report: &a3s_cloud_contracts::NodeLogChunkReport,
    ) -> std::result::Result<
        crate::modules::fleet::domain::services::StoredLogChunk,
        crate::modules::fleet::domain::services::LogChunkStoreError,
    > {
        Ok(crate::modules::fleet::domain::services::StoredLogChunk {
            object_key: format!("test/{ordinal}"),
            created: false,
        })
    }

    async fn remove(
        &self,
        _object_key: &str,
    ) -> std::result::Result<(), crate::modules::fleet::domain::services::LogChunkStoreError> {
        Ok(())
    }

    async fn health(
        &self,
    ) -> std::result::Result<bool, crate::modules::fleet::domain::services::LogChunkStoreError>
    {
        Ok(true)
    }
}

#[async_trait::async_trait]
impl ICertificateAuthority for TestCertificateAuthority {
    async fn issue(
        &self,
        request: NodeCertificateRequest,
    ) -> std::result::Result<NodeCertificate, CertificateAuthorityError> {
        NodeCertificate::new(
            request.certificate_id,
            request.node_id,
            NodeCertificateMaterial {
                serial_number: request.certificate_id.to_string(),
                fingerprint: format!("sha256:{:x}", Sha256::digest(request.csr_pem.as_bytes())),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at: request.issued_at,
                expires_at: request.expires_at,
            },
        )
        .map_err(CertificateAuthorityError::InvalidRequest)
    }

    async fn revoke(
        &self,
        _certificate: &NodeCertificate,
    ) -> std::result::Result<(), CertificateAuthorityError> {
        Ok(())
    }

    async fn health(&self) -> std::result::Result<bool, CertificateAuthorityError> {
        Ok(true)
    }
}

fn config() -> CloudConfig {
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
            certificate_file: ".a3s/test-security/node-control/server.pem".into(),
            private_key_file: ".a3s/test-security/node-control/server-key.pem".into(),
            client_ca_file: ".a3s/test-security/node-ca/ca.pem".into(),
            max_request_bytes: 20 * 1024 * 1024,
            tls_handshake_timeout_ms: 5_000,
            request_body_timeout_ms: 10_000,
        },
        postgres: PostgresConfig {
            url_env: "A3S_CLOUD_POSTGRES_URL".into(),
            max_connections: 4,
        },
        auth: AuthConfig {
            bootstrap_token_env: "A3S_CLOUD_BOOTSTRAP_TOKEN".into(),
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
            state_dir: ".a3s/test-security".into(),
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

fn post_json(path: impl Into<String>, idempotency_key: &str, body: Value) -> BootRequest {
    post_json_as(path, idempotency_key, body, ADMIN_TOKEN)
}

fn post_json_as(
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

fn delete_as(path: impl Into<String>, idempotency_key: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Delete, path.into())
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
}

fn get_as(path: impl Into<String>, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Get, path.into())
        .with_header("accept", "application/json")
        .with_header("authorization", format!("Bearer {token}"))
}

fn build_test_application(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
) -> Result<BootApplication> {
    let nodes = Arc::new(InMemoryNodeRepository::new());
    build_application_with_health(
        config(),
        ApplicationDependencies {
            organizations: identity.clone(),
            api_tokens: identity,
            projects: projects.clone(),
            environments: projects,
            workloads: Arc::new(InMemoryWorkloadRepository::new()),
            operations: Arc::new(InMemoryOperationRepository::new()),
            nodes: nodes.clone(),
            node_control: nodes,
            log_chunks: Arc::new(TestLogChunkStore),
            certificate_authority: Arc::new(TestCertificateAuthority),
            bootstrap_credential: BootstrapCredential::new(BOOTSTRAP_TOKEN)
                .map_err(BootError::Internal)?,
            readiness: HealthModule::new("readiness")
                .with_route("/health/ready")
                .indicator("repositories", || async { Ok(HealthIndicatorResult::up()) }),
        },
    )
}

fn response_json(response: &BootResponse) -> Result<Value> {
    response.body_json()
}

fn response_id(response: &BootResponse) -> Result<String> {
    response_json(response)?["data"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal("response does not contain a resource ID".into()))
}

async fn create_organization(
    app: &BootApplication,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            "/api/v1/organizations",
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}

async fn bootstrap_organization(
    app: &BootApplication,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(
            post_json(
                "/api/v1/bootstrap",
                idempotency_key,
                json!({
                    "organizationName": name,
                    "tokenName": "bootstrap-admin",
                    "token": ADMIN_TOKEN,
                    "expiresAt": null
                }),
            )
            .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN),
        )
        .await?;
    assert_eq!(response.status(), 201);
    response_json(&response)?["data"]["organization"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal("bootstrap response has no organization ID".into()))
}

async fn create_project(
    app: &BootApplication,
    organization_id: &str,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/projects"),
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}

async fn create_api_token(
    app: &BootApplication,
    organization_id: &str,
    idempotency_key: &str,
    name: &str,
    secret: &str,
    scopes: &[&str],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/api-tokens"),
            idempotency_key,
            json!({
                "name": name,
                "token": secret,
                "scopes": scopes,
                "expiresAt": expires_at,
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    assert!(!String::from_utf8_lossy(response.body()).contains(secret));
    response_id(&response)
}

fn runtime_capabilities() -> Value {
    json!({
        "schema": "a3s.runtime.capabilities.v2",
        "provider_id": "docker",
        "provider_build": "test",
        "unit_classes": ["task", "service"],
        "artifact_media_types": ["application/vnd.oci.image.manifest.v1+json"],
        "isolation_levels": ["container"],
        "network_modes": ["none", "service"],
        "mount_kinds": [],
        "health_check_kinds": [],
        "resource_controls": ["cpu", "memory", "pids", "ephemeral_storage"],
        "features": ["durable_identity", "stop", "remove"]
    })
}

#[tokio::test]
async fn boot_shell_exposes_wrapped_platform_and_health_responses() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects)?;
    let platform = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/platform")
                .with_header("accept", "application/json")
                .with_header("x-request-id", "018f3f56-8d4a-7c2a-9f13-5ab3d245d701"),
        )
        .await?;
    let body = response_json(&platform)?;
    assert_eq!(platform.status(), 200);
    assert_eq!(body["code"], 200);
    assert_eq!(body["data"]["name"], "a3s-cloud");
    assert_eq!(body["requestId"], "018f3f56-8d4a-7c2a-9f13-5ab3d245d701");

    let health = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/health/live")
                .with_header("accept", "application/json"),
        )
        .await?;
    let body = response_json(&health)?;
    assert_eq!(body["data"]["status"], "up");

    let readiness = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/health/ready")
                .with_header("accept", "application/json"),
        )
        .await?;
    let body = response_json(&readiness)?;
    assert_eq!(body["data"]["status"], "up");
    Ok(())
}

#[tokio::test]
async fn organization_writes_are_idempotent_unique_and_atomic() -> Result<()> {
    let repository = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(repository.clone(), projects)?;
    bootstrap_organization(&app, "bootstrap-root", "Root").await?;
    let request = || {
        post_json(
            "/api/v1/organizations",
            "create-acme",
            json!({"name": "Acme"}),
        )
    };

    let first = app.call(request()).await?;
    let second = app.call(request()).await?;
    let first_body = response_json(&first)?;
    let second_body = response_json(&second)?;
    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 200);
    assert_eq!(first_body["data"]["id"], second_body["data"]["id"]);
    assert_eq!(second_body["data"]["replayed"], true);

    let changed = app
        .call(post_json(
            "/api/v1/organizations",
            "create-acme",
            json!({"name": "Other"}),
        ))
        .await?;
    assert_eq!(changed.status(), 409);
    assert_eq!(response_json(&changed)?["statusCode"], "CONFLICT");

    let duplicate = app
        .call(post_json(
            "/api/v1/organizations",
            "duplicate-acme",
            json!({"name": "acme"}),
        ))
        .await?;
    assert_eq!(duplicate.status(), 409);
    assert_eq!(repository.outbox_events().await.len(), 3);
    Ok(())
}

#[tokio::test]
async fn project_writes_are_idempotent_and_names_are_organization_scoped() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects.clone())?;
    let acme = bootstrap_organization(&app, "organization-acme", "Acme").await?;
    let beta = create_organization(&app, "organization-beta", "Beta").await?;
    let path = format!("/api/v1/organizations/{acme}/projects");
    let request = || post_json(&path, "project-cloud", json!({"name": "Cloud"}));

    let first = app.call(request()).await?;
    let second = app.call(request()).await?;
    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 200);
    assert_eq!(response_id(&first)?, response_id(&second)?);
    assert_eq!(response_json(&second)?["data"]["replayed"], true);

    let changed = app
        .call(post_json(&path, "project-cloud", json!({"name": "Other"})))
        .await?;
    assert_eq!(changed.status(), 409);

    let duplicate = app
        .call(post_json(
            &path,
            "project-cloud-duplicate",
            json!({"name": "cloud"}),
        ))
        .await?;
    assert_eq!(duplicate.status(), 409);

    let other_scope = app
        .call(post_json(
            format!("/api/v1/organizations/{beta}/projects"),
            "project-cloud",
            json!({"name": "Cloud"}),
        ))
        .await?;
    assert_eq!(other_scope.status(), 201);
    let events = projects.outbox_events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "project.project.created")
            .count(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn environment_writes_are_idempotent_and_names_are_project_scoped() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects.clone())?;
    let organization = bootstrap_organization(&app, "organization", "Acme").await?;
    let cloud = create_project(&app, &organization, "project-cloud", "Cloud").await?;
    let data = create_project(&app, &organization, "project-data", "Data").await?;
    let path = format!("/api/v1/organizations/{organization}/projects/{cloud}/environments");
    let request = || {
        post_json(
            &path,
            "environment-production",
            json!({"name": "Production"}),
        )
    };

    let first = app.call(request()).await?;
    let second = app.call(request()).await?;
    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 200);
    assert_eq!(response_id(&first)?, response_id(&second)?);
    assert_eq!(response_json(&second)?["data"]["replayed"], true);

    let changed = app
        .call(post_json(
            &path,
            "environment-production",
            json!({"name": "Staging"}),
        ))
        .await?;
    assert_eq!(changed.status(), 409);

    let duplicate = app
        .call(post_json(
            &path,
            "environment-production-duplicate",
            json!({"name": "production"}),
        ))
        .await?;
    assert_eq!(duplicate.status(), 409);

    let other_scope = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{data}/environments"),
            "environment-production",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(other_scope.status(), 201);
    let events = projects.outbox_events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "project.environment.created")
            .count(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn projects_and_environments_reject_cross_tenant_references() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects.clone())?;
    let organization_id = bootstrap_organization(&app, "organization", "Acme").await?;
    let project_id = create_project(&app, &organization_id, "project", "Cloud").await?;

    let wrong_organization = Uuid::new_v4();
    let rejected = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{wrong_organization}/projects/{project_id}/environments"
            ),
            "wrong-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    let rejected_body = response_json(&rejected)?;
    assert_eq!(rejected.status(), 404);
    assert_eq!(rejected_body["statusCode"], "NOT_FOUND");

    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/projects/{project_id}/environments"),
            "environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    assert_eq!(projects.outbox_events().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn bearer_tokens_are_scoped_to_one_organization_and_never_echoed() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let acme = bootstrap_organization(&app, "bootstrap-acme", "Acme").await?;
    let beta = create_organization(&app, "organization-beta", "Beta").await?;
    create_api_token(
        &app,
        &acme,
        "token-projects",
        "project-automation",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let no_credentials = app
        .call(
            BootRequest::new(
                HttpMethod::Post,
                format!("/api/v1/organizations/{acme}/projects"),
            )
            .with_header("content-type", "application/json")
            .with_header("idempotency-key", "unauthenticated")
            .with_body(json!({"name": "Rejected"}).to_string().into_bytes()),
        )
        .await?;
    assert_eq!(no_credentials.status(), 401);

    let own_project = app
        .call(post_json_as(
            format!("/api/v1/organizations/{acme}/projects"),
            "project-own",
            json!({"name": "Own"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(own_project.status(), 201);

    let cross_tenant = app
        .call(post_json_as(
            format!("/api/v1/organizations/{beta}/projects"),
            "project-cross-tenant",
            json!({"name": "Rejected"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant.status(), 403);

    let scope_escalation = app
        .call(post_json_as(
            format!("/api/v1/organizations/{acme}/api-tokens"),
            "scope-escalation",
            json!({
                "name": "Escalated",
                "token": EXPIRING_TOKEN,
                "scopes": [ApiTokenScope::TOKEN_WRITE],
                "expiresAt": null
            }),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(scope_escalation.status(), 403);
    Ok(())
}

#[tokio::test]
async fn revoked_and_expired_tokens_stop_authenticating_immediately() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "bootstrap", "Acme").await?;
    let project_token_id = create_api_token(
        &app,
        &organization,
        "token-revoked",
        "revoked-token",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;
    let revoke_path = format!("/api/v1/organizations/{organization}/api-tokens/{project_token_id}");
    let revoked = app
        .call(delete_as(&revoke_path, "revoke-project-token", ADMIN_TOKEN))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert!(response_json(&revoked)?["data"]["revokedAt"].is_string());
    let replayed = app
        .call(delete_as(&revoke_path, "revoke-project-token", ADMIN_TOKEN))
        .await?;
    assert_eq!(response_json(&replayed)?["data"]["replayed"], true);

    let revoked_use = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/projects"),
            "revoked-use",
            json!({"name": "Rejected"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(revoked_use.status(), 401);

    create_api_token(
        &app,
        &organization,
        "token-expiring",
        "expiring-token",
        EXPIRING_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        Some(chrono::Utc::now() + chrono::Duration::milliseconds(40)),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(60)).await;
    let expired_use = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/projects"),
            "expired-use",
            json!({"name": "Rejected"}),
            EXPIRING_TOKEN,
        ))
        .await?;
    assert_eq!(expired_use.status(), 401);
    Ok(())
}

#[tokio::test]
async fn fleet_api_enrolls_lists_and_changes_node_state_without_exposing_secrets() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "fleet-bootstrap", "Acme").await?;
    create_api_token(
        &app,
        &organization,
        "fleet-limited-token",
        "project-only",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let enrollment_secret = format!("a3sn_{}", "d".repeat(64));
    let token_path = format!("/api/v1/organizations/{organization}/enrollment-tokens");
    let forbidden = app
        .call(post_json_as(
            &token_path,
            "fleet-token-forbidden",
            json!({
                "name": "worker",
                "token": enrollment_secret,
                "expiresAt": Utc::now() + chrono::Duration::minutes(10)
            }),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(forbidden.status(), 403);
    let issued = app
        .call(post_json(
            &token_path,
            "fleet-token",
            json!({
                "name": "worker",
                "token": enrollment_secret,
                "expiresAt": Utc::now() + chrono::Duration::minutes(10)
            }),
        ))
        .await?;
    assert_eq!(issued.status(), 201);
    assert!(!String::from_utf8_lossy(issued.body()).contains(&enrollment_secret));

    let agent_instance_id = Uuid::now_v7();
    let enrolled = app
            .call(
                BootRequest::new(HttpMethod::Post, "/api/v1/node-control/enroll")
                    .with_header("content-type", "application/json")
                    .with_body(
                        json!({
                            "schema": "a3s.cloud.node-enrollment-request.v1",
                            "enrollment_token": enrollment_secret,
                            "node_name": "worker-1",
                            "agent_instance_id": agent_instance_id,
                            "agent_version": "0.1.0",
                            "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\ndGVzdA==\n-----END CERTIFICATE REQUEST-----\n",
                            "runtime_capabilities": runtime_capabilities()
                        })
                        .to_string()
                        .into_bytes(),
                    ),
            )
            .await?;
    assert_eq!(enrolled.status(), 201);
    let enrollment = response_json(&enrolled)?;
    assert_eq!(
        enrollment["schema"],
        "a3s.cloud.node-enrollment-response.v1"
    );
    let node_id = enrollment["node_id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("enrollment response has no node ID".into()))?;

    let nodes_path = format!("/api/v1/organizations/{organization}/nodes");
    let listed = app.call(get_as(&nodes_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(response_json(&listed)?["data"][0]["state"], "pending");
    let node_path = format!("{nodes_path}/{node_id}");
    let found = app.call(get_as(&node_path, ADMIN_TOKEN)).await?;
    assert_eq!(response_json(&found)?["data"]["name"], "worker-1");

    let drain_path = format!("{node_path}/actions/drain");
    let drained = app
        .call(post_json(
            &drain_path,
            "fleet-drain",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(drained.status(), 200);
    assert_eq!(response_json(&drained)?["data"]["state"], "draining");
    let drain_replay = app
        .call(post_json(
            &drain_path,
            "fleet-drain",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(response_json(&drain_replay)?["data"]["replayed"], true);
    let revoked = app
        .call(post_json(
            format!("{node_path}/actions/revoke"),
            "fleet-revoke",
            json!({"expectedVersion": 2}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_eq!(response_json(&revoked)?["data"]["state"], "revoked");
    Ok(())
}

#[tokio::test]
async fn authenticated_queries_and_operation_stream_return_authoritative_snapshots() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "project", "Cloud").await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);

    let organizations = app
        .call(get_as("/api/v1/organizations", ADMIN_TOKEN))
        .await?;
    assert_eq!(response_json(&organizations)?["data"][0]["name"], "Acme");
    let listed_projects = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/projects"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&listed_projects)?["data"][0]["name"], "Cloud");
    let environments = app.call(get_as(&environment_path, ADMIN_TOKEN)).await?;
    assert_eq!(
        response_json(&environments)?["data"][0]["name"],
        "Production"
    );
    let operations = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/operations"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&operations)?["data"], json!([]));

    let stream = app
        .call(
            BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/organizations/{organization}/operations/stream"),
            )
            .with_header("accept", "text/event-stream")
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
    assert_eq!(stream.status(), 200);
    assert!(stream.is_streaming());
    assert!(stream.is_event_stream());
    Ok(())
}
