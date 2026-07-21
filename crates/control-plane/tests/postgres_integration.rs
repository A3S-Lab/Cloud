use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod};
use a3s_cloud_control_plane::app::build_application_with_source_resolver;
use a3s_cloud_control_plane::config::{
    AuthConfig, DeploymentsConfig, EdgeConfig, EventProviderKind, EventsConfig, FleetConfig,
    LogsConfig, NodeControlConfig, OperationsConfig, PostgresConfig, ProcessRole, RegistryConfig,
    SecurityConfig, SecurityProfile, SecurityProviderKind, ServerConfig, SourcesConfig,
};
use a3s_cloud_control_plane::infrastructure::FlowInfrastructure;
use a3s_cloud_control_plane::modules::integration_events::{
    A3sEventPublisher, OutboxRelay, OutboxRelayConfig, PostgresOutboxRepository,
};
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationRepository, OperationRequest, OperationStatus, OperationSubject,
    PostgresOperationRepository, RebuildOperationProjectionsHandler, ReconcileOperationsHandler,
    WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{OperationId, OrganizationId};
use a3s_cloud_control_plane::modules::sources::domain::{
    GitReference, ISourceResolver, ResolvedSource, SourceProviderCredential, SourceResolutionError,
    SourceResolutionRequest,
};
use a3s_cloud_control_plane::{
    build_application, infrastructure::connect_and_migrate, CloudConfig,
};
use a3s_event::{NatsConfig, StorageType};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use a3s_orm::{sql_query, Database, Migration, Migrator, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::Utc;
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[path = "support/activation_retirement_crash.rs"]
mod activation_retirement_crash_support;
#[path = "support/cancellation.rs"]
mod cancellation_support;
#[path = "support/deployment_flow.rs"]
mod deployment_flow_support;
#[path = "support/edge_certificate_lifecycle.rs"]
mod edge_certificate_lifecycle_support;
#[path = "support/edge.rs"]
mod edge_support;
#[path = "support/fleet.rs"]
mod fleet_support;
#[path = "support/github_connection.rs"]
mod github_connection_support;
#[path = "support/postgres_fixture.rs"]
mod postgres_fixture;
#[path = "support/secret_rotation_provider_crash.rs"]
mod secret_rotation_provider_crash_support;
#[path = "support/secret_rotation_restart.rs"]
mod secret_rotation_restart_support;
#[path = "support/source_subscription.rs"]
mod source_subscription_support;
#[path = "support/workload_rollback.rs"]
mod workload_rollback_support;
#[path = "support/workloads.rs"]
mod workloads_support;

use postgres_fixture::*;

struct OfflineCommitSourceResolver;

#[async_trait]
impl ISourceResolver for OfflineCommitSourceResolver {
    async fn resolve(
        &self,
        request: &SourceResolutionRequest,
        _credential: Option<&SourceProviderCredential>,
    ) -> Result<ResolvedSource, SourceResolutionError> {
        let GitReference::Commit(commit_sha) = &request.reference else {
            return Err(SourceResolutionError::Unavailable);
        };
        Ok(ResolvedSource {
            repository: request.repository.clone(),
            commit_sha: commit_sha.clone(),
        })
    }
}

#[tokio::test]
#[ignore = "private subprocess used only by the activation-before-retirement crash gate"]
async fn activation_before_retirement_crash_probe() {
    activation_retirement_crash_support::run_activation_crash_probe()
        .await
        .expect("run activation-before-retirement crash probe");
}

#[tokio::test]
#[ignore = "private subprocess used only by the PostgreSQL log recovery acceptance gate"]
async fn log_object_publish_crash_probe() {
    deployment_flow_support::run_log_object_publish_crash_probe()
        .await
        .expect("run log object publish crash probe");
}

#[tokio::test]
#[ignore = "private subprocess used only by the Secret-rotation provider crash gate"]
async fn secret_rotation_provider_crash_probe() {
    secret_rotation_provider_crash_support::run_provider_crash_probe()
        .await
        .expect("run Secret-rotation provider crash probe");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_foundation_is_migrated_atomic_and_idempotent(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return Ok(());
    };
    let isolated = IsolatedPostgresDatabase::create(&admin_url).await?;
    let result = AssertUnwindSafe(exercise_postgres_foundation(isolated.url().to_owned()))
        .catch_unwind()
        .await;
    let cleanup = isolated.cleanup().await;

    match result {
        Ok(Ok(())) => cleanup,
        Ok(Err(test_error)) => {
            if let Err(cleanup_error) = cleanup {
                return Err(std::io::Error::other(format!(
                    "PostgreSQL integration test failed: {test_error}; isolated database cleanup also failed: {cleanup_error}"
                ))
                .into());
            }
            Err(test_error)
        }
        Err(panic_payload) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!(
                    "isolated PostgreSQL database cleanup failed after test panic: {cleanup_error}"
                );
            }
            std::panic::resume_unwind(panic_payload)
        }
    }
}

async fn exercise_postgres_foundation(url: String) -> Result<(), Box<dyn std::error::Error>> {
    let admin = PostgresExecutor::connect_no_tls(&url, 4)?;
    admin
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop schema if exists a3s_flow cascade;
             drop table if exists github_connection_lifecycle_inbox cascade;
             drop table if exists github_repository_subscriptions cascade;
             drop table if exists github_source_connections cascade;
             drop table if exists github_connection_flows cascade;
             drop table if exists source_webhook_inbox cascade;
             drop table if exists source_webhook_deliveries cascade;
             drop table if exists external_source_revisions cascade;
             drop table if exists secret_rotation_reconciliations cascade;
             drop table if exists secret_rotation_restarts cascade;
             drop table if exists secret_versions cascade;
             drop table if exists secrets cascade;
             drop table if exists gateway_certificate_convergences cascade;
             drop table if exists gateway_route_cutovers cascade;
             drop table if exists deployments cascade;
             drop table if exists workload_revisions cascade;
             drop table if exists workloads cascade;
             drop table if exists routes cascade;
             drop table if exists gateway_certificates cascade;
             drop table if exists domain_claims cascade;
             drop table if exists gateway_publications cascade;
             drop table if exists gateway_scopes cascade;
             drop table if exists node_gateway_acknowledgements cascade;
             drop table if exists node_log_compaction_ranges cascade;
             drop table if exists node_log_batch_chunks cascade;
             drop table if exists node_log_chunks cascade;
             drop table if exists node_log_batches cascade;
             drop table if exists node_log_chunk_receipts cascade;
             drop table if exists runtime_observations cascade;
             drop table if exists node_commands cascade;
             drop table if exists node_certificate_rotations cascade;
             drop table if exists node_certificates cascade;
             drop table if exists node_enrollment_reservations cascade;
             drop table if exists nodes cascade;
             drop table if exists enrollment_tokens cascade;
             drop table if exists api_tokens cascade;
             drop table if exists operation_projections cascade;
             drop table if exists operation_requests cascade;
             drop table if exists audit_records cascade;
             drop table if exists outbox_events cascade;
             drop table if exists idempotency_records cascade;
             drop table if exists environments cascade;
             drop table if exists projects cascade;
             drop table if exists organizations cascade;
             drop table if exists a3s_orm_rollback_probe cascade;
             drop table if exists a3s_orm_migrations cascade;
             drop function if exists reject_cloud_outbox() cascade;
             drop function if exists reject_outbox_ack() cascade;",
        )
        .await?;

    let (left, right) = tokio::join!(connect_and_migrate(&url, 4), connect_and_migrate(&url, 4));
    let executor = left?;
    right?;
    let database = Database::new(PostgresDialect, executor.clone());
    let applied = database
        .fetch_one_as(sql_query::<i64>("select count(*) from a3s_orm_migrations"))
        .await?;
    assert_eq!(applied, 25);
    let route_ownership_predicate = database
        .fetch_one_as(sql_query::<String>(
            "select pg_get_expr(indpred, indrelid) from pg_index where indexrelid = 'routes_active_ownership_idx'::regclass",
        ))
        .await?;
    assert!(route_ownership_predicate.contains("'publishing'"));
    assert!(route_ownership_predicate.contains("'active'"));
    let permanent_route_ownership = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conname = 'routes_gateway_node_id_hostname_path_prefix_key'",
        ))
        .await?;
    assert_eq!(permanent_route_ownership, 0);
    let deployment_version_checks = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conrelid = 'deployments'::regclass and contype = 'c' and pg_get_constraintdef(oid) like '%aggregate_version%'",
        ))
        .await?;
    assert_eq!(deployment_version_checks, 1);

    let drift = Migrator::new(executor.clone())
        .run([Migration::new("001", "changed", "select 1")])
        .await;
    assert!(drift.is_err());
    assert!(drift
        .err()
        .is_some_and(|error| error.to_string().contains("changed after it was applied")));

    let failed = Migrator::new(executor.clone())
        .run([
            Migration::new(
                "001",
                "cloud foundation",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/001_foundation.sql"
                )),
            ),
            Migration::new(
                "002",
                "flow operations",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/002_flow_operations.sql"
                )),
            ),
            Migration::new(
                "003",
                "outbox leases",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/003_outbox_leases.sql"
                )),
            ),
            Migration::new(
                "004",
                "API tokens",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/004_api_tokens.sql"
                )),
            ),
            Migration::new(
                "005",
                "fleet node control",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/005_fleet.sql"
                )),
            ),
            Migration::new(
                "006",
                "workloads and deployments",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/006_workloads.sql"
                )),
            ),
            Migration::new(
                "007",
                "deployment cancellation cleanup",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/007_deployment_cleanup.sql"
                )),
            ),
            Migration::new(
                "008",
                "workload revision resolution",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/008_workload_revision_resolution.sql"
                )),
            ),
            Migration::new(
                "009",
                "same-generation Runtime apply recovery",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/009_runtime_apply_recovery.sql"
                )),
            ),
            Migration::new(
                "010",
                "Gateway snapshot commands",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/010_gateway_snapshot_commands.sql"
                )),
            ),
            Migration::new(
                "011",
                "Edge route publications",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/011_edge_routes.sql"
                )),
            ),
            Migration::new(
                "012",
                "Edge domain ownership and TLS certificates",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/012_edge_tls.sql"
                )),
            ),
            Migration::new(
                "013",
                "encrypted Secret resources",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/013_secrets.sql"
                )),
            ),
            Migration::new(
                "014",
                "durable log retention tombstones",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/014_log_retention.sql"
                )),
            ),
            Migration::new(
                "015",
                "bounded log tombstone compaction",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/015_log_tombstone_compaction.sql"
                )),
            ),
            Migration::new(
                "016",
                "durable provider log gaps",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/016_provider_log_gaps.sql"
                )),
            ),
            Migration::new(
                "017",
                "Secret rotation workload restarts",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/017_secret_rotation_restarts.sql"
                )),
            ),
            Migration::new(
                "018",
                "Gateway route cutovers",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/018_gateway_route_cutovers.sql"
                )),
            ),
            Migration::new(
                "019",
                "deployment retirement",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/019_deployment_retirement.sql"
                )),
            ),
            Migration::new(
                "020",
                "Gateway certificate convergence",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/020_gateway_certificate_convergence.sql"
                )),
            ),
            Migration::new(
                "021",
                "external source revisions",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/021_external_source_revisions.sql"
                )),
            ),
            Migration::new(
                "022",
                "source webhook inbox",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/022_source_webhook_inbox.sql"
                )),
            ),
            Migration::new(
                "023",
                "GitHub source connections",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/023_github_source_connections.sql"
                )),
            ),
            Migration::new(
                "024",
                "GitHub repository subscriptions",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/024_github_repository_subscriptions.sql"
                )),
            ),
            Migration::new(
                "025",
                "GitHub connection lifecycle",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/025_github_connection_lifecycle.sql"
                )),
            ),
            Migration::new(
                "026",
                "broken migration",
                "create table a3s_orm_rollback_probe (id bigint); invalid sql",
            ),
        ])
        .await;
    assert!(failed.is_err());
    let rollback_probe = database
        .fetch_one_as(sql_query::<Option<String>>(
            "select to_regclass('public.a3s_orm_rollback_probe')::text",
        ))
        .await?;
    assert_eq!(rollback_probe, None);

    let _postgres_url = EnvironmentOverride::set(URL_ENV, &url);
    let _bootstrap_token = EnvironmentOverride::set(BOOTSTRAP_ENV, BOOTSTRAP_TOKEN);
    let _github_webhook_secret =
        EnvironmentOverride::set(GITHUB_WEBHOOK_ENV, GITHUB_WEBHOOK_SECRET);
    let security_directory = tempfile::tempdir()?;
    let mut application_config = config();
    application_config.security.state_dir = security_directory.path().display().to_string();
    application_config.node_control.certificate_file = security_directory
        .path()
        .join("node-control/server.pem")
        .display()
        .to_string();
    application_config.node_control.private_key_file = security_directory
        .path()
        .join("node-control/server-key.pem")
        .display()
        .to_string();
    application_config.node_control.client_ca_file = security_directory
        .path()
        .join("node-ca/ca.pem")
        .display()
        .to_string();
    let app = if std::env::var("A3S_CLOUD_TEST_OFFLINE_SOURCE_RESOLVER").as_deref() == Ok("1") {
        build_application_with_source_resolver(
            application_config,
            Arc::new(OfflineCommitSourceResolver),
        )
        .await?
    } else {
        build_application(application_config).await?
    };
    let readiness = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/health/ready")
                .with_header("accept", "application/json"),
        )
        .await?;
    assert_eq!(readiness.status(), 200);

    let organization_request = || {
        post_json(
            "/api/v1/bootstrap",
            "organization-acme",
            json!({
                "organizationName": "Acme",
                "tokenName": "bootstrap-admin",
                "token": ADMIN_TOKEN,
                "expiresAt": null
            }),
        )
        .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN)
    };
    let (first, replay) = tokio::join!(
        app.call(organization_request()),
        app.call(organization_request())
    );
    let first = first?;
    let replay = replay?;
    let mut statuses = [first.status(), replay.status()];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 201]);
    let organization_id = response_json(&first)?["data"]["organization"]["id"]
        .as_str()
        .ok_or("first bootstrap response has no organization ID")?
        .to_owned();
    assert_eq!(
        response_json(&replay)?["data"]["organization"]["id"],
        organization_id
    );

    let changed = app
        .call(
            post_json(
                "/api/v1/bootstrap",
                "organization-acme",
                json!({
                    "organizationName": "Other",
                    "tokenName": "bootstrap-admin",
                    "token": ADMIN_TOKEN,
                    "expiresAt": null
                }),
            )
            .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN),
        )
        .await?;
    assert_eq!(changed.status(), 409);

    let project_path = format!("/api/v1/organizations/{organization_id}/projects");
    let project = app
        .call(post_json(
            &project_path,
            "project-cloud",
            json!({"name": "Cloud"}),
        ))
        .await?;
    let project_replay = app
        .call(post_json(
            &project_path,
            "project-cloud",
            json!({"name": "Cloud"}),
        ))
        .await?;
    assert_eq!(project.status(), 201);
    assert_eq!(project_replay.status(), 200);
    assert_eq!(response_id(&project)?, response_id(&project_replay)?);
    let project_id = response_id(&project)?;

    let environment_path =
        format!("/api/v1/organizations/{organization_id}/projects/{project_id}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "environment-production",
            json!({"name": "Production"}),
        ))
        .await?;
    let environment_replay = app
        .call(post_json(
            &environment_path,
            "environment-production",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    assert_eq!(environment_replay.status(), 200);
    assert_eq!(
        response_id(&environment)?,
        response_id(&environment_replay)?
    );
    let environment_id = response_id(&environment)?;

    let installation_conflict_organization = app
        .call(post_json(
            "/api/v1/organizations",
            "organization-github-installation-conflict",
            json!({"name": "GitHub installation conflict"}),
        ))
        .await?;
    assert_eq!(installation_conflict_organization.status(), 201);
    let account_conflict_organization = app
        .call(post_json(
            "/api/v1/organizations",
            "organization-github-account-conflict",
            json!({"name": "GitHub account conflict"}),
        ))
        .await?;
    assert_eq!(account_conflict_organization.status(), 201);
    github_connection_support::exercise_github_connection_persistence(
        &executor,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        OrganizationId::from_uuid(Uuid::parse_str(&response_id(
            &installation_conflict_organization,
        )?)?),
        OrganizationId::from_uuid(Uuid::parse_str(&response_id(
            &account_conflict_organization,
        )?)?),
    )
    .await?;

    let webhook_body = serde_json::to_vec(&json!({
        "ref": "refs/heads/main",
        "after": "7b7c8152cc148688b403a489a9866731b2e92063",
        "deleted": false,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42}
    }))?;
    let webhook = app
        .call(github_webhook_request(
            "push",
            "postgres-webhook-a",
            &webhook_body,
        ))
        .await?;
    let webhook_replay = app
        .call(github_webhook_request(
            "push",
            "postgres-webhook-a",
            &webhook_body,
        ))
        .await?;
    assert_eq!(webhook.status(), 202);
    assert_eq!(webhook_replay.status(), 202);
    let changed_webhook_body = serde_json::to_vec(&json!({
        "ref": "refs/heads/main",
        "after": "52b6a42b75f7e8405ddb2cab1c8f9c4285302a57",
        "deleted": false,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42}
    }))?;
    let webhook_conflict = app
        .call(github_webhook_request(
            "push",
            "postgres-webhook-a",
            &changed_webhook_body,
        ))
        .await?;
    assert_eq!(webhook_conflict.status(), 409);
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from source_webhook_inbox",
            ))
            .await?,
        1
    );

    let cross_tenant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{}/projects/{project_id}/environments",
                Uuid::new_v4()
            ),
            "cross-tenant",
            json!({"name": "Rejected"}),
        ))
        .await?;
    assert_eq!(cross_tenant.status(), 404);

    let source_path = format!(
        "/api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions"
    );
    let source_request = |repository: &str, commit_sha: &str| {
        json!({
            "repository": {
                "provider": "github",
                "url": repository
            },
            "reference": {
                "kind": "commit",
                "value": commit_sha
            },
            "recipe": {
                "schema": "a3s.cloud.build-recipe.v1",
                "kind": "dockerfile",
                "contextPath": "./services/api",
                "dockerfilePath": "Dockerfile",
                "target": "release",
                "platforms": ["linux/arm64", "linux/amd64"]
            },
            "webhookDeliveryId": "postgres-delivery-a"
        })
    };
    let commit_a = "7b7c8152cc148688b403a489a9866731b2e92063";
    let source = app
        .call(post_json(
            &source_path,
            "source-revision-a",
            source_request("https://github.com/A3S-Lab/Cloud.git", commit_a),
        ))
        .await?;
    let source_replay = app
        .call(post_json(
            &source_path,
            "source-revision-a",
            source_request("https://github.com/A3S-Lab/Cloud.git", commit_a),
        ))
        .await?;
    let source_canonical_duplicate = app
        .call(post_json(
            &source_path,
            "source-revision-a-canonical",
            source_request(
                "https://GITHUB.com/a3s-lab/cloud/",
                &commit_a.to_uppercase(),
            ),
        ))
        .await?;
    assert_eq!(source.status(), 201);
    assert_eq!(source_replay.status(), 200);
    assert_eq!(source_canonical_duplicate.status(), 200);
    assert_eq!(response_id(&source)?, response_id(&source_replay)?);
    assert_eq!(
        response_id(&source)?,
        response_id(&source_canonical_duplicate)?
    );
    let moved_delivery = app
        .call(post_json(
            &source_path,
            "source-revision-moved-delivery",
            source_request(
                "https://github.com/a3s-lab/cloud",
                "52b6a42b75f7e8405ddb2cab1c8f9c4285302a57",
            ),
        ))
        .await?;
    assert_eq!(moved_delivery.status(), 409);
    let listed_sources = app.call(get_as(&source_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed_sources.status(), 200);
    assert_eq!(
        response_json(&listed_sources)?["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let source_rows = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from external_source_revisions",
        ))
        .await?;
    let delivery_rows = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from source_webhook_deliveries",
        ))
        .await?;
    let source_events = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from outbox_events where event_key = 'source.revision.accepted'",
        ))
        .await?;
    assert_eq!(source_rows, 1);
    assert_eq!(delivery_rows, 1);
    assert_eq!(source_events, 1);

    source_subscription_support::exercise_source_subscriptions(
        &app,
        &executor,
        &organization_id,
        &project_id,
        &environment_id,
    )
    .await?;

    let secrets_path = format!(
        "/api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/secrets"
    );
    let first_secret_value = "postgres://cloud:first-secret@database";
    let create_secret = || {
        post_json(
            &secrets_path,
            "secret-database-url",
            json!({"name": "Database URL", "value": first_secret_value}),
        )
    };
    let secret = app.call(create_secret()).await?;
    let secret_replay = app.call(create_secret()).await?;
    assert_eq!(secret.status(), 201);
    assert_eq!(secret_replay.status(), 200);
    assert_eq!(response_id(&secret)?, response_id(&secret_replay)?);
    assert!(!String::from_utf8_lossy(secret.body()).contains(first_secret_value));
    let secret_id = response_id(&secret)?;
    let secret_versions_path =
        format!("/api/v1/organizations/{organization_id}/secrets/{secret_id}/versions");
    let second_secret_value = "postgres://cloud:rotated-secret@database";
    let rotated_secret = app
        .call(post_json(
            &secret_versions_path,
            "secret-database-url-rotate",
            json!({"value": second_secret_value}),
        ))
        .await?;
    assert_eq!(rotated_secret.status(), 201);
    assert_eq!(response_json(&rotated_secret)?["data"]["currentVersion"], 2);
    assert!(!String::from_utf8_lossy(rotated_secret.body()).contains(second_secret_value));
    let revoked_secret_version = app
        .call(post_json(
            format!("{secret_versions_path}/1/revoke"),
            "secret-database-url-revoke-v1",
            json!({}),
        ))
        .await?;
    assert_eq!(revoked_secret_version.status(), 200);
    assert_eq!(
        response_json(&revoked_secret_version)?["data"]["version"]["state"],
        "revoked"
    );
    let registry_username = std::env::var("A3S_CLOUD_TEST_REGISTRY_USERNAME")
        .unwrap_or_else(|_| "registry-user".into());
    let registry_password = std::env::var("A3S_CLOUD_TEST_REGISTRY_PASSWORD")
        .unwrap_or_else(|_| "registry-password".into());
    let registry_credential_value = json!({
        "schema": "a3s.cloud.registry-credential.v1",
        "username": registry_username,
        "password": registry_password,
    })
    .to_string();
    let registry_secret = app
        .call(post_json(
            &secrets_path,
            "secret-registry-credential",
            json!({"name": "Registry credential", "value": registry_credential_value}),
        ))
        .await?;
    assert_eq!(registry_secret.status(), 201);
    assert!(!String::from_utf8_lossy(registry_secret.body())
        .contains(registry_credential_value.as_str()));
    let registry_secret_id = response_id(&registry_secret)?;
    let leaked_secret_rows = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from secret_versions where ciphertext like ")
                .bind(format!("%{first_secret_value}%"))
                .append(" or ciphertext like ")
                .bind(format!("%{second_secret_value}%"))
                .append(" or ciphertext like ")
                .bind(format!("%{registry_credential_value}%")),
        )
        .await?;
    assert_eq!(leaked_secret_rows, 0);
    let encrypted_secret_rows = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from secret_versions where key_id like 'local:sha256:%' and octet_length(ciphertext) > 32",
        ))
        .await?;
    assert_eq!(encrypted_secret_rows, 3);
    let safe_secret_idempotency = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from idempotency_records where idempotency_key like 'secret-database-url%' and (select count(*) from jsonb_object_keys(response)) = 2 and response ->> 'secret_id' is not null and response ->> 'version' is not null and response::text not like '%ciphertext%' and response::text not like '%key_id%'",
        ))
        .await?;
    assert_eq!(safe_secret_idempotency, 3);
    let safe_secret_events = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from outbox_events where event_key like 'secret.%' and payload::text not like '%ciphertext%' and payload::text not like '%key_id%'",
        ))
        .await?;
    assert_eq!(safe_secret_events, 4);
    let leaked_secret_metadata = database
        .fetch_one_as(
            sql_query::<i64>(
                "select (select count(*) from outbox_events where payload::text like ",
            )
            .bind(format!("%{first_secret_value}%"))
            .append(" or payload::text like ")
            .bind(format!("%{second_secret_value}%"))
            .append(" or payload::text like ")
            .bind(format!("%{registry_credential_value}%"))
            .append(") + (select count(*) from idempotency_records where response::text like ")
            .bind(format!("%{first_secret_value}%"))
            .append(" or response::text like ")
            .bind(format!("%{second_secret_value}%"))
            .append(" or response::text like ")
            .bind(format!("%{registry_credential_value}%"))
            .append(")"),
        )
        .await?;
    assert_eq!(leaked_secret_metadata, 0);

    let token_path = format!("/api/v1/organizations/{organization_id}/api-tokens");
    let project_token = app
        .call(post_json(
            &token_path,
            "token-projects",
            json!({
                "name": "project-automation",
                "token": PROJECT_TOKEN,
                "scopes": ["project:write"],
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(project_token.status(), 201);
    assert!(!String::from_utf8_lossy(project_token.body()).contains(PROJECT_TOKEN));
    let project_token_id = response_id(&project_token)?;
    let plaintext_token_rows = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from api_tokens where token_hash = ")
                .bind(PROJECT_TOKEN),
        )
        .await?;
    let hashed_token_rows = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from api_tokens where token_hash like 'sha256:%'",
        ))
        .await?;
    assert_eq!(plaintext_token_rows, 0);
    assert_eq!(hashed_token_rows, 2);

    let own_project = app
        .call(post_json_as(
            &project_path,
            "project-limited-token",
            json!({"name": "Limited"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(own_project.status(), 201);
    let tenant_guard = app
        .call(post_json_as(
            format!("/api/v1/organizations/{}/projects", Uuid::new_v4()),
            "project-other-tenant",
            json!({"name": "Rejected"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(tenant_guard.status(), 403);

    let revoke_path = format!("{token_path}/{project_token_id}");
    let revoked = app
        .call(delete_as(&revoke_path, "revoke-project-token", ADMIN_TOKEN))
        .await?;
    assert_eq!(revoked.status(), 200);
    let revoked_use = app
        .call(post_json_as(
            &project_path,
            "revoked-token-use",
            json!({"name": "Rejected"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(revoked_use.status(), 401);

    let expiring_token = app
        .call(post_json(
            &token_path,
            "token-expiring",
            json!({
                "name": "expiring",
                "token": EXPIRING_TOKEN,
                "scopes": ["project:write"],
                "expiresAt": Utc::now() + chrono::Duration::seconds(1)
            }),
        ))
        .await?;
    assert_eq!(expiring_token.status(), 201);
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let expired_use = app
        .call(post_json_as(
            &project_path,
            "expired-token-use",
            json!({"name": "Rejected"}),
            EXPIRING_TOKEN,
        ))
        .await?;
    assert_eq!(expired_use.status(), 401);

    let outbox_events = database
        .fetch_one_as(sql_query::<i64>("select count(*) from outbox_events"))
        .await?;
    let idempotency_records = database
        .fetch_one_as(sql_query::<i64>("select count(*) from idempotency_records"))
        .await?;
    assert_eq!(outbox_events, 22);
    assert_eq!(idempotency_records, 19);

    let operation_id = OperationId::new();
    let operation_request = OperationRequest::new(
        operation_id,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        OperationSubject::new("deployment", Uuid::now_v7())?,
        WorkflowIdentity::new("cloud.deployment", "2")?,
        json!({"generation": 1}),
        Utc::now(),
    );
    let operation_repository = Arc::new(PostgresOperationRepository::new(executor.clone()));
    let (enqueued, enqueue_replay) = tokio::join!(
        operation_repository.enqueue(operation_request.clone()),
        operation_repository.enqueue(operation_request.clone())
    );
    let enqueued = enqueued?;
    let enqueue_replay = enqueue_replay?;
    assert_ne!(enqueued.replayed, enqueue_replay.replayed);

    let flow = FlowInfrastructure::connect(&url, Arc::new(CompletingRuntime)).await?;
    assert!(flow.engine().list_run_ids().await?.is_empty());
    let operation_engine = Arc::new(FlowOperationEngine::new(flow.engine()));
    let reconciler =
        ReconcileOperationsHandler::new(operation_repository.clone(), operation_engine.clone());
    let (left, right) = tokio::join!(reconciler.execute(10), reconciler.execute(10));
    assert!(left?.failures.is_empty());
    assert!(right?.failures.is_empty());
    assert_eq!(
        flow.engine().list_run_ids().await?,
        vec![operation_id.to_string()]
    );
    assert_eq!(
        flow.engine()
            .history(&operation_id.to_string())
            .await?
            .len(),
        3
    );
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("operation projection was not written")?
            .status,
        OperationStatus::Succeeded
    );
    assert_eq!(reconciler.execute(10).await?.inspected, 0);

    database
        .execute(
            sql_query::<()>("delete from operation_projections where operation_id = ")
                .bind(operation_id.as_uuid()),
        )
        .await?;
    let rebuilder =
        RebuildOperationProjectionsHandler::new(operation_repository.clone(), operation_engine);
    let rebuild = rebuilder.execute().await?;
    assert_eq!(rebuild.rebuilt, 1);
    assert!(rebuild.orphaned.is_empty());
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("operation projection was not rebuilt")?
            .status,
        OperationStatus::Succeeded
    );
    let flow_events = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from a3s_flow.flow_events",
        ))
        .await?;
    assert_eq!(flow_events, 3);

    let memory_publisher = Arc::new(A3sEventPublisher::memory());
    let memory_bus = memory_publisher.bus();
    let relay = OutboxRelay::new(
        Arc::new(PostgresOutboxRepository::new(executor.clone())),
        memory_publisher,
        OutboxRelayConfig {
            batch_size: 100,
            poll_interval: Duration::from_millis(10),
            lease_duration: Duration::from_millis(100),
            publish_timeout: Duration::from_millis(50),
            initial_backoff: Duration::from_millis(1),
            maximum_backoff: Duration::from_millis(10),
        },
    )?;
    let delivered = relay.run_once().await?;
    let initial_event_count = usize::try_from(outbox_events)?;
    assert_eq!(delivered.claimed, initial_event_count);
    assert_eq!(delivered.published, initial_event_count);
    assert!(delivered.failures.is_empty());
    assert_eq!(relay.run_once().await?.claimed, 0);

    let relay_crash = app
        .call(post_json(
            "/api/v1/organizations",
            "organization-relay-crash",
            json!({"name": "RelayCrash"}),
        ))
        .await?;
    assert_eq!(relay_crash.status(), 201);
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "create function reject_outbox_ack() returns trigger language plpgsql as $$
               begin
                 if new.published_at is not null and new.payload ->> 'name' = 'RelayCrash' then
                   raise exception 'injected outbox acknowledgement failure';
                 end if;
                 return new;
               end
             $$;
             create trigger reject_outbox_ack before update of published_at on outbox_events
               for each row execute function reject_outbox_ack();",
        )
        .await?;
    let lost_ack = relay.run_once().await?;
    assert_eq!(lost_ack.claimed, 1);
    assert_eq!(lost_ack.published, 0);
    assert_eq!(lost_ack.failures.len(), 1);
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop trigger reject_outbox_ack on outbox_events;
             drop function reject_outbox_ack();",
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(relay.run_once().await?.published, 1);
    let local_events = memory_bus.list_events(None, 100).await?;
    assert_eq!(local_events.len(), initial_event_count + 2);
    let unique_event_ids = local_events
        .iter()
        .map(|event| event.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_event_ids.len(), initial_event_count + 1);

    if let Ok(nats_url) = std::env::var("A3S_CLOUD_TEST_NATS_URL") {
        let nats_created = app
            .call(post_json(
                "/api/v1/organizations",
                "organization-nats-crash",
                json!({"name": "NatsCrash"}),
            ))
            .await?;
        assert_eq!(nats_created.status(), 201);
        let nats_config = NatsConfig {
            url: nats_url,
            stream_name: format!("A3S_CLOUD_TEST_{}", Uuid::new_v4().simple()).to_uppercase(),
            subject_prefix: format!("a3s_cloud_test_{}", Uuid::new_v4().simple()).to_lowercase(),
            storage: StorageType::Memory,
            ..NatsConfig::default()
        };
        let nats_subject = format!("{}.cloud.>", nats_config.subject_prefix);
        let nats_publisher = Arc::new(A3sEventPublisher::nats(nats_config).await?);
        let nats_bus = nats_publisher.bus();
        let mut subscription = nats_bus.provider().subscribe(&nats_subject).await?;
        let nats_relay = OutboxRelay::new(
            Arc::new(PostgresOutboxRepository::new(executor.clone())),
            nats_publisher,
            OutboxRelayConfig {
                batch_size: 10,
                poll_interval: Duration::from_millis(10),
                lease_duration: Duration::from_secs(2),
                publish_timeout: Duration::from_secs(1),
                initial_backoff: Duration::from_millis(1),
                maximum_backoff: Duration::from_millis(10),
            },
        )?;
        executor
            .pool()
            .get()
            .await?
            .batch_execute(
                "create function reject_outbox_ack() returns trigger language plpgsql as $$
                   begin
                     if new.published_at is not null and new.payload ->> 'name' = 'NatsCrash' then
                       raise exception 'injected NATS outbox acknowledgement failure';
                     end if;
                     return new;
                   end
                 $$;
                 create trigger reject_outbox_ack before update of published_at on outbox_events
                   for each row execute function reject_outbox_ack();",
            )
            .await?;
        let first_attempt = nats_relay.run_once().await?;
        assert_eq!(first_attempt.claimed, 1);
        assert_eq!(first_attempt.failures.len(), 1);
        executor
            .pool()
            .get()
            .await?
            .batch_execute(
                "drop trigger reject_outbox_ack on outbox_events;
                 drop function reject_outbox_ack();",
            )
            .await?;
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(nats_relay.run_once().await?.published, 1);
        let received = tokio::time::timeout(Duration::from_secs(2), subscription.next())
            .await??
            .ok_or("NATS subscription closed before receiving the event")?;
        assert_eq!(received.event.event_type, "identity.organization.created");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), subscription.next())
                .await
                .is_err()
        );
        assert_eq!(nats_bus.info().await?.messages, 1);
    }

    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "create function reject_cloud_outbox() returns trigger language plpgsql as $$
               begin
                 if new.payload ->> 'name' = 'Rollback' then
                   raise exception 'injected outbox failure';
                 end if;
                 return new;
               end
             $$;
             create trigger reject_cloud_outbox before insert on outbox_events
               for each row execute function reject_cloud_outbox();",
        )
        .await?;
    let rolled_back = app
        .call(post_json(
            "/api/v1/organizations",
            "organization-rollback",
            json!({"name": "Rollback"}),
        ))
        .await?;
    assert_eq!(rolled_back.status(), 500);
    let stored_organization = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from organizations where name_key = ")
                .bind("rollback"),
        )
        .await?;
    let stored_idempotency = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from idempotency_records where idempotency_key = ")
                .bind("organization-rollback"),
        )
        .await?;
    assert_eq!(stored_organization, 0);
    assert_eq!(stored_idempotency, 0);

    let workload_path = format!(
        "/api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workloads"
    );
    let private_registry_artifact = std::env::var("A3S_CLOUD_TEST_PRIVATE_REGISTRY_ARTIFACT").ok();
    let artifact_uri = private_registry_artifact.as_deref().unwrap_or(
        "oci://docker.io/library/busybox@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662",
    );
    let artifact_digest = artifact_uri
        .rsplit_once('@')
        .map(|(_, digest)| digest)
        .filter(|digest| {
            digest.len() == 71
                && digest.starts_with("sha256:")
                && digest[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .ok_or("workload fixture artifact is not digest-pinned")?;
    let mut workload_secrets = vec![
        json!({
            "name": "database-url-environment",
            "secretId": secret_id,
            "version": 2,
            "target": {
                "kind": "environment",
                "variable": "DATABASE_URL"
            }
        }),
        json!({
            "name": "database-url-file",
            "secretId": secret_id,
            "version": 2,
            "target": {
                "kind": "file",
                "path": "/run/secrets/database-url",
                "mode": 256
            }
        }),
    ];
    if private_registry_artifact.is_some() {
        workload_secrets.push(json!({
            "name": "registry-credential",
            "secretId": registry_secret_id,
            "version": 1,
            "target": {
                "kind": "registry_credential"
            }
        }));
    }
    let workload_body = json!({
        "name": "API fixture",
        "template": {
            "artifact": {
                "uri": artifact_uri,
                "expectedDigest": artifact_digest
            },
            "process": {
                "command": ["/bin/sh"],
                "args": ["-c", "set -eu; file_value=$(cat /run/secrets/database-url); test \"$DATABASE_URL\" = \"$file_value\"; printf 'env-secret=%s\\n' \"$DATABASE_URL\"; printf 'file-secret=%s\\n' \"$file_value\" >&2; printf 'log-recovery-probe\\n'; mkdir -p /www; printf 'healthy\\n' >/www/index.html; exec httpd -f -p 8080 -h /www"],
                "workingDirectory": null,
                "environment": {}
            },
            "secrets": workload_secrets,
            "resources": {
                "cpuMillis": 250,
                "memoryBytes": 67108864,
                "pids": 64,
                "ephemeralStorageBytes": null
            },
            "ports": [{"name": "http", "containerPort": 8080}],
            "health": {
                "portName": "http",
                "path": "/",
                "intervalMs": 100,
                "timeoutMs": 100,
                "healthyThreshold": 2,
                "unhealthyThreshold": 20,
                "stabilizationWindowMs": 100
            }
        }
    });
    let created_workload = app
        .call(post_json(
            &workload_path,
            "api-workload-fixture",
            workload_body.clone(),
        ))
        .await?;
    let replayed_workload = app
        .call(post_json(
            &workload_path,
            "api-workload-fixture",
            workload_body.clone(),
        ))
        .await?;
    assert_eq!(created_workload.status(), 202);
    assert_eq!(replayed_workload.status(), 200);
    assert_eq!(
        response_json(&created_workload)?["data"]["deploymentId"],
        response_json(&replayed_workload)?["data"]["deploymentId"]
    );
    assert_eq!(response_json(&replayed_workload)?["data"]["replayed"], true);
    let changed_workload = app
        .call(post_json(
            &workload_path,
            "api-workload-fixture",
            json!({"name": "Changed", "template": workload_body["template"].clone()}),
        ))
        .await?;
    assert_eq!(changed_workload.status(), 409);

    let created_workload_body = response_json(&created_workload)?;
    let workload_id = created_workload_body["data"]["workloadId"]
        .as_str()
        .ok_or("workload creation response omitted workloadId")?
        .to_owned();
    let deployment_id = created_workload_body["data"]["deploymentId"]
        .as_str()
        .ok_or("workload creation response omitted deploymentId")?
        .to_owned();
    let revision_id = created_workload_body["data"]["revisionId"]
        .as_str()
        .ok_or("workload creation response omitted revisionId")?
        .to_owned();
    let sensitive_plaintexts = [
        second_secret_value,
        registry_credential_value.as_str(),
        registry_password.as_str(),
    ];
    let deployment_flow_fixture = deployment_flow_support::exercise_deployment_flow(
        &executor,
        &url,
        Uuid::parse_str(&organization_id)?,
        &response_json(&created_workload)?["data"],
        security_directory.path(),
        &sensitive_plaintexts,
    )
    .await?;
    let third_secret_value = "postgres://cloud:restart-secret@database";
    let restart_rotation = app
        .call(post_json(
            &secret_versions_path,
            "secret-database-url-rotate-for-restart",
            json!({"value": third_secret_value}),
        ))
        .await?;
    assert_eq!(restart_rotation.status(), 201);
    assert_eq!(
        response_json(&restart_rotation)?["data"]["currentVersion"],
        3
    );
    assert!(!String::from_utf8_lossy(restart_rotation.body()).contains(third_secret_value));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from secret_versions where secret_id = ",)
                    .bind(Uuid::parse_str(&secret_id)?)
                    .append(" and version = 3"),
            )
            .await?,
        1,
        "Secret rotation did not commit before restart reconciliation"
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workload_revisions where workload_id = ",)
                    .bind(Uuid::parse_str(&workload_id)?),
            )
            .await?,
        1,
        "restart revision appeared in the Secret mutation transaction"
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from secret_rotation_restarts where workload_id = ",
                )
                .bind(Uuid::parse_str(&workload_id)?),
            )
            .await?,
        0,
        "restart intent appeared before the committed Secret event was reconciled"
    );

    let restart_fixture = secret_rotation_restart_support::exercise_secret_rotation_restart(
        &executor,
        &url,
        Uuid::parse_str(&organization_id)?,
        Uuid::parse_str(&workload_id)?,
        Uuid::parse_str(&secret_id)?,
        3,
        &deployment_flow_fixture,
        security_directory.path(),
        &[
            second_secret_value,
            third_secret_value,
            registry_credential_value.as_str(),
            registry_password.as_str(),
        ],
    )
    .await?;
    for plaintext in [
        second_secret_value,
        third_secret_value,
        registry_credential_value.as_str(),
        registry_password.as_str(),
    ] {
        let durable_leaks = database
            .fetch_one_as(
                sql_query::<i64>("with needle as (select ")
                    .bind(format!("%{plaintext}%"))
                    .append(
                        "::text as value) select
                         (select count(*) from workload_revisions, needle
                            where template_request::text like needle.value
                               or coalesce(template::text, '') like needle.value
                               or request_digest like needle.value
                               or coalesce(template_digest, '') like needle.value)
                       + (select count(*) from secret_rotation_restarts, needle
                            where row_to_json(secret_rotation_restarts)::text like needle.value)
                       + (select count(*) from secret_rotation_reconciliations, needle
                            where row_to_json(secret_rotation_reconciliations)::text like needle.value)
                       + (select count(*) from secret_versions, needle
                            where ciphertext like needle.value
                               or key_id like needle.value)
                       + (select count(*) from operation_requests, needle
                            where input::text like needle.value)
                       + (select count(*) from operation_projections, needle
                            where coalesce(output::text, '') like needle.value
                               or coalesce(error, '') like needle.value)
                       + (select count(*) from a3s_flow.flow_events, needle
                            where event_json like needle.value)
                       + (select count(*) from node_commands, needle
                            where payload::text like needle.value
                               or coalesce(acknowledgement::text, '') like needle.value)
                       + (select count(*) from runtime_observations, needle
                            where observation::text like needle.value)
                       + (select count(*) from outbox_events, needle
                            where payload::text like needle.value
                               or coalesce(last_error, '') like needle.value)
                       + (select count(*) from audit_records, needle
                            where details::text like needle.value)
                       + (select count(*) from idempotency_records, needle
                            where response::text like needle.value)",
                    ),
            )
            .await?;
        assert_eq!(
            durable_leaks, 0,
            "plaintext Secret reached durable control-plane state"
        );
    }

    if std::env::var("A3S_CLOUD_TEST_DOCKER").as_deref() == Ok("1") {
        let persisted_logs = app
            .call(get_as(
                format!(
                    "/api/v1/organizations/{organization_id}/workloads/{workload_id}/revisions/{revision_id}/logs?limit=32"
                ),
                ADMIN_TOKEN,
            ))
            .await?;
        assert_eq!(persisted_logs.status(), 200);
        let persisted_logs_body = String::from_utf8_lossy(persisted_logs.body());
        assert!(!persisted_logs_body.contains(first_secret_value));
        assert!(!persisted_logs_body.contains(second_secret_value));
        assert!(!persisted_logs_body.contains(third_secret_value));
        assert!(!persisted_logs_body.contains(registry_credential_value.as_str()));
        assert!(!persisted_logs_body.contains(registry_password.as_str()));
        let persisted_logs_json = response_json(&persisted_logs)?;
        let records = persisted_logs_json["data"]["records"]
            .as_array()
            .ok_or("persisted workload logs response omitted records")?;
        let log_recovery = deployment_flow_fixture
            .log_recovery
            .as_ref()
            .ok_or("Docker deployment fixture omitted log recovery evidence")?;
        let corrupt_record = records
            .iter()
            .find(|record| record["sequence"].as_u64() == Some(log_recovery.corrupted_sequence))
            .ok_or("workload log response omitted the corrupted object sequence")?;
        assert_eq!(corrupt_record["kind"], "gap");
        assert_eq!(corrupt_record["gapReason"], "corrupt");
        assert_eq!(corrupt_record["stream"], log_recovery.corrupted_stream);
        assert!(corrupt_record["data"].is_null());
        assert_eq!(
            records
                .iter()
                .filter(|record| record["gapReason"] == "corrupt")
                .count(),
            1
        );
        assert!(records.iter().all(|record| {
            !record["data"]
                .as_str()
                .is_some_and(|data| data.contains("log-recovery-probe"))
        }));
        assert!(records.iter().any(|record| {
            record["stream"] == "stdout"
                && record["data"]
                    .as_str()
                    .is_some_and(|data| data.contains("env-secret=[REDACTED]"))
        }));
        assert!(records.iter().any(|record| {
            record["stream"] == "stderr"
                && record["data"]
                    .as_str()
                    .is_some_and(|data| data.contains("file-secret=[REDACTED]"))
        }));
    }
    let listed_workloads = app.call(get_as(&workload_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed_workloads.status(), 200);
    let listed = &response_json(&listed_workloads)?["data"];
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    assert_eq!(listed[0]["id"], workload_id);
    assert_eq!(listed[0]["desiredRevision"]["generation"], 2);
    assert_eq!(listed[0]["activeRevision"]["generation"], 2);
    assert_eq!(listed[0]["deployments"][0]["status"], "active");
    assert_eq!(
        listed[0]["deployments"][0]["observedRuntime"]["state"],
        "running"
    );
    assert_eq!(
        listed[0]["deployments"][0]["observedRuntime"]["healthState"],
        "healthy"
    );

    let workload_detail = app
        .call(get_as(
            format!("/api/v1/organizations/{organization_id}/workloads/{workload_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(workload_detail.status(), 200);
    assert_eq!(response_json(&workload_detail)?["data"]["id"], workload_id);

    let deployment_detail = app
        .call(get_as(
            format!("/api/v1/organizations/{organization_id}/deployments/{deployment_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(deployment_detail.status(), 200);
    assert_eq!(
        response_json(&deployment_detail)?["data"]["id"],
        deployment_id
    );
    let restart_revision_id = restart_fixture.revision_id.to_string();
    let restart_deployment_id = restart_fixture.deployment_id.to_string();
    let restart_operation_id = restart_fixture.operation_id.to_string();
    let restart_deployment_detail = app
        .call(get_as(
            format!("/api/v1/organizations/{organization_id}/deployments/{restart_deployment_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(restart_deployment_detail.status(), 200);
    assert_eq!(
        response_json(&restart_deployment_detail)?["data"]["operationId"],
        restart_operation_id
    );
    for response in [
        &listed_workloads,
        &workload_detail,
        &deployment_detail,
        &restart_deployment_detail,
    ] {
        let body = String::from_utf8_lossy(response.body());
        assert!(!body.contains(second_secret_value));
        assert!(!body.contains(third_secret_value));
        assert!(!body.contains(registry_credential_value.as_str()));
        assert!(!body.contains(registry_password.as_str()));
    }

    edge_support::exercise_edge_api(
        &app,
        &executor,
        edge_support::EdgeApiFixture {
            organization_id: &organization_id,
            project_id: &project_id,
            environment_id: &environment_id,
            workload_revision_id: &restart_revision_id,
            token: ADMIN_TOKEN,
        },
    )
    .await?;

    let mut cancellation_workload_body = workload_body;
    cancellation_workload_body["template"]["secrets"] = json!([]);
    cancellation_workload_body["template"]["process"]["args"] = json!([
        "-c",
        "mkdir -p /www && printf 'healthy\\n' >/www/index.html && exec httpd -f -p 8080 -h /www"
    ]);
    cancellation_support::exercise_deployment_cancellation(
        cancellation_support::CancellationScenario {
            app: &app,
            executor: &executor,
            postgres_url: &url,
            organization_id: &organization_id,
            workload_path: &workload_path,
            workload_body: cancellation_workload_body,
            active_deployment_id: &restart_deployment_id,
            admin_token: ADMIN_TOKEN,
        },
    )
    .await?;

    let rollback_replay = workload_rollback_support::accept_and_cancel(
        workload_rollback_support::RollbackApiScenario {
            app: &app,
            executor: &executor,
            organization_id: &organization_id,
            workload_id: &workload_id,
            source_revision_id: &revision_id,
            current_revision_id: &restart_revision_id,
            artifact_digest,
            token: ADMIN_TOKEN,
        },
    )
    .await?;

    let stop_path = format!("/api/v1/organizations/{organization_id}/workloads/{workload_id}/stop");
    let stop = app
        .call(post_json(&stop_path, "api-stop-workload", json!({})))
        .await?;
    let stop_replay = app
        .call(post_json(&stop_path, "api-stop-workload", json!({})))
        .await?;
    assert_eq!(stop.status(), 202);
    assert_eq!(stop_replay.status(), 200);
    assert_eq!(response_json(&stop)?["data"]["desiredState"], "stopped");
    assert_eq!(response_json(&stop_replay)?["data"]["replayed"], true);
    assert_eq!(
        response_json(&stop)?["data"]["operationId"],
        response_json(&stop_replay)?["data"]["operationId"]
    );
    let stopped_detail = app
        .call(get_as(
            format!("/api/v1/organizations/{organization_id}/workloads/{workload_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(
        response_json(&stopped_detail)?["data"]["desiredState"],
        "stopped"
    );
    assert_eq!(
        response_json(&stopped_detail)?["data"]["activeRevision"]["generation"],
        2
    );
    workload_rollback_support::assert_replay_after_workload_stop(
        &app,
        rollback_replay,
        ADMIN_TOKEN,
    )
    .await?;

    fleet_support::exercise_fleet(&executor, Uuid::parse_str(&organization_id)?).await?;
    let workload_fixture = workloads_support::exercise_workloads(
        &executor,
        Uuid::parse_str(&organization_id)?,
        Uuid::parse_str(&project_id)?,
        Uuid::parse_str(&environment_id)?,
    )
    .await?;
    edge_support::exercise_edge(
        &executor,
        edge_support::EdgeFixture {
            organization_id: OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
            project_id:
                a3s_cloud_control_plane::modules::shared_kernel::domain::ProjectId::from_uuid(
                    Uuid::parse_str(&project_id)?,
                ),
            environment_id:
                a3s_cloud_control_plane::modules::shared_kernel::domain::EnvironmentId::from_uuid(
                    Uuid::parse_str(&environment_id)?,
                ),
            node_id: workload_fixture.node_id,
            workload_id: workload_fixture.workload_id,
            revision_id: workload_fixture.revision_id,
            candidate_revision_id: workload_fixture.candidate_revision_id,
            candidate_deployment_id: workload_fixture.candidate_deployment_id,
        },
    )
    .await?;

    Ok(())
}
