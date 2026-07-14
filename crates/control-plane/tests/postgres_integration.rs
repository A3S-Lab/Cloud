use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod};
use a3s_cloud_control_plane::config::{
    AuthConfig, EventProviderKind, EventsConfig, OperationsConfig, PostgresConfig, ProcessRole,
    ServerConfig,
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
use a3s_cloud_control_plane::{
    build_application, infrastructure::connect_and_migrate, CloudConfig,
};
use a3s_event::{NatsConfig, StorageType};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use a3s_orm::{sql_query, Database, Migration, Migrator, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const URL_ENV: &str = "A3S_CLOUD_INTEGRATION_POSTGRES_URL";
const BOOTSTRAP_ENV: &str = "A3S_CLOUD_INTEGRATION_BOOTSTRAP_TOKEN";
const BOOTSTRAP_TOKEN: &str = "integration-bootstrap-credential-0123456789abcdef";
const ADMIN_TOKEN: &str = "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROJECT_TOKEN: &str = "a3s_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EXPIRING_TOKEN: &str = "a3s_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Debug, Clone, Copy)]
struct CompletingRuntime;

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

fn response_json(response: &BootResponse) -> a3s_boot::Result<Value> {
    response.body_json()
}

fn response_id(response: &BootResponse) -> a3s_boot::Result<String> {
    response_json(response)?["data"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal("response does not contain a resource ID".into()))
}

fn config() -> CloudConfig {
    CloudConfig {
        server: ServerConfig {
            host: "127.0.0.1".into(),
            port: 8080,
            role: ProcessRole::All,
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
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_foundation_is_migrated_atomic_and_idempotent(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return Ok(());
    };
    let admin = PostgresExecutor::connect_no_tls(&url, 4)?;
    admin
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop schema if exists a3s_flow cascade;
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
    assert_eq!(applied, 4);

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

    std::env::set_var(URL_ENV, &url);
    std::env::set_var(BOOTSTRAP_ENV, BOOTSTRAP_TOKEN);
    let app = build_application(config()).await?;
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
                "expiresAt": Utc::now() + chrono::Duration::milliseconds(40)
            }),
        ))
        .await?;
    assert_eq!(expiring_token.status(), 201);
    tokio::time::sleep(Duration::from_millis(60)).await;
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
    assert_eq!(outbox_events, 8);
    assert_eq!(idempotency_records, 7);

    let operation_id = OperationId::new();
    let operation_request = OperationRequest::new(
        operation_id,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        OperationSubject::new("deployment", Uuid::now_v7())?,
        WorkflowIdentity::new("cloud.deployment", "1")?,
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
    assert_eq!(delivered.claimed, 8);
    assert_eq!(delivered.published, 8);
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
    assert_eq!(local_events.len(), 10);
    let unique_event_ids = local_events
        .iter()
        .map(|event| event.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_event_ids.len(), 9);

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
    std::env::remove_var(URL_ENV);
    std::env::remove_var(BOOTSTRAP_ENV);
    Ok(())
}
