use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod, QueueOptions};
use a3s_cloud_control_plane::app::build_application_with_source_resolver;
use a3s_cloud_control_plane::config::{
    ArtifactTransferConfig, AssetsConfig, AuthConfig, BuildsConfig, DeploymentsConfig, EdgeConfig,
    EventProviderKind, EventsConfig, FleetConfig, HumanTasksConfig, LogsConfig, NodeControlConfig,
    OperationsConfig, PostgresConfig, ProcessRole, RegistryConfig, SecurityConfig, SecurityProfile,
    SecurityProviderKind, ServerConfig, SourcesConfig,
};
use a3s_cloud_control_plane::infrastructure::{FlowInfrastructure, FlowOperationCoordinator};
use a3s_cloud_control_plane::modules::assets::{
    AcquireAssetGitWriteLease, Asset, AssetCreated, AssetGitRpcLimits, AssetGitService,
    AssetGitWriteOperation, AssetGitWriteRecovery, AssetKind, ClaimAssetGitWriteRecovery,
    CreateAssetWrite, IAssetGitRepository, IAssetGitRepositoryControl, IAssetRepository,
    LocalAssetGitRepository, PostgresAssetRepository,
};
use a3s_cloud_control_plane::modules::integration_events::{
    A3sEventPublisher, OutboxRelay, OutboxRelayConfig, PostgresOutboxRepository,
};
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationRepository, OperationReconciler, OperationRequest,
    OperationStatus, OperationSubject, PostgresOperationRepository,
    RebuildOperationProjectionsHandler, ReconcileOperationsHandler, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetId, IdempotencyRequest, OperationId, OrganizationId, ProjectId, ResourceName,
};
use a3s_cloud_control_plane::modules::sources::domain::{
    GitReference, ISourceResolver, ResolvedSource, SourceProviderCredential, SourceResolutionError,
    SourceResolutionRequest,
};
use a3s_cloud_control_plane::{
    build_application, infrastructure::connect_and_migrate, CloudConfig,
};
use a3s_event::{NatsConfig, StorageType};
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
};
use a3s_orm::{
    sql_query, Database, Migration, Migrator, PostgresDialect, PostgresError, PostgresExecutor,
};
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
#[path = "support/assets.rs"]
mod assets_support;
#[path = "support/build_evidence.rs"]
mod build_evidence_support;
#[path = "support/build_flow_process_death.rs"]
mod build_flow_process_death_support;
#[path = "support/build_runs.rs"]
mod build_runs_support;
#[path = "support/cancellation.rs"]
mod cancellation_support;
#[path = "support/deployment_flow.rs"]
mod deployment_flow_support;
#[path = "support/edge_certificate_lifecycle.rs"]
mod edge_certificate_lifecycle_support;
#[path = "support/edge.rs"]
mod edge_support;
#[path = "support/executions.rs"]
mod executions_support;
#[path = "support/fleet.rs"]
mod fleet_support;
#[path = "support/forms.rs"]
mod forms_support;
#[path = "support/g0_external_release.rs"]
mod g0_external_release_support;
#[path = "support/gateway_replica_recovery.rs"]
mod gateway_replica_recovery_support;
#[path = "support/gateway_rollouts.rs"]
mod gateway_rollouts_support;
#[path = "support/github_connection.rs"]
mod github_connection_support;
#[path = "support/human_tasks.rs"]
mod human_tasks_support;
#[path = "support/mcp_route_policies.rs"]
mod mcp_route_policies_support;
#[path = "support/plugins.rs"]
mod plugins_support;
#[path = "support/postgres_fixture.rs"]
mod postgres_fixture;
#[path = "support/resource_claims.rs"]
mod resource_claims_support;
#[path = "support/secret_rotation_restart.rs"]
mod secret_rotation_restart_support;
#[path = "support/source_subscription.rs"]
mod source_subscription_support;
#[path = "support/workflow_run_process_death.rs"]
mod workflow_run_process_death_support;
#[path = "support/workload_rollback.rs"]
mod workload_rollback_support;
#[path = "support/workloads.rs"]
mod workloads_support;

use postgres_fixture::*;

const ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));

fn integration_breaking_ontology_acl(compatible_acl: &str) -> String {
    let changed = compatible_acl.replacen(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        1,
    );
    let body = changed
        .strip_suffix("}\n")
        .expect("public Ontology fixture must end with its root block");
    format!(
        "{body}\n  rule \"migrate_ticket_v2\" {{\n    label = \"Migrate ticket v2\"\n    kind = \"migration\"\n    expression_digest = \"sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"\n  }}\n}}\n"
    )
}

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
#[ignore = "private subprocess used only by the persistent Build Flow process-death gate"]
async fn build_flow_postgres_process_death_probe() {
    build_flow_process_death_support::run_probe()
        .await
        .expect("run persistent Build Flow process-death probe");
}

#[tokio::test]
#[ignore = "private subprocess used only by the WorkflowRun process-death gate"]
async fn workflow_run_postgres_process_death_probe() {
    workflow_run_process_death_support::run_probe()
        .await
        .expect("run WorkflowRun process-death probe");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_build_flow_survives_process_death_at_every_fleet_completion_boundary() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(
        &admin_url,
        build_flow_process_death_support::exercise_process_death_matrix,
    )
    .await
    .expect("persistent Build Flow process-death gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_workflow_run_survives_api_and_worker_process_death() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(
        &admin_url,
        workflow_run_process_death_support::exercise_process_death_matrix,
    )
    .await
    .expect("WorkflowRun API and worker process-death gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires private GitHub, exact A3S Box, external Registry, and Vault Transit providers"]
async fn postgres_g0_external_release_persists_verified_evidence_and_workload_handoff() {
    let Some(admin_url) = std::env::var(g0_external_release_support::POSTGRES_ENV).ok() else {
        return;
    };
    run_isolated_postgres(
        &admin_url,
        g0_external_release_support::exercise_external_release,
    )
    .await
    .expect("G0 external release provider gate");
}

#[test]
fn postgres_foundation_is_migrated_atomic_and_idempotent() {
    const STACK_SIZE: usize = 32 * 1024 * 1024;
    let result = std::thread::Builder::new()
        .name("postgres-foundation".into())
        .stack_size(STACK_SIZE)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .thread_stack_size(STACK_SIZE)
                .enable_all()
                .build()
                .map_err(|error| format!("build PostgreSQL test runtime: {error}"))?
                .block_on(run_postgres_foundation_test())
                .map_err(|error| error.to_string())
        })
        .expect("spawn PostgreSQL foundation test thread")
        .join();
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("{error}"),
        Err(panic_payload) => std::panic::resume_unwind(panic_payload),
    }
}

async fn run_postgres_foundation_test() -> Result<(), Box<dyn std::error::Error>> {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return Ok(());
    };
    run_isolated_postgres(&admin_url, exercise_postgres_foundation).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_replica_set_foundation_is_migrated_atomic_and_replay_safe() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_postgres_replica_set_foundation)
        .await
        .expect("PostgreSQL Workload replica-set foundation gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_plugin_registry_is_atomic_tenant_scoped_and_searchable() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(
        &admin_url,
        plugins_support::exercise_plugin_registry_persistence,
    )
    .await
    .expect("PostgreSQL Plugin Registry persistence gate");
}

async fn exercise_postgres_replica_set_foundation(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let (left, right) = tokio::join!(connect_and_migrate(&url, 4), connect_and_migrate(&url, 4));
    let executor = left?;
    right?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(sql_query::<(i64, String)>(
            "select count(*), max(version) from a3s_orm_migrations",
        ))
        .await?;
    assert_eq!(migration_state, (92, "092".into()));

    let organization_id = Uuid::now_v7();
    let project_id = Uuid::now_v7();
    let environment_id = Uuid::now_v7();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", 'Replica-set tenant', 'replica-set-tenant', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", ")
            .bind(project_id)
            .append(", 'Replica-set project', 'replica-set-project', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", ")
            .bind(project_id)
            .append(", ")
            .bind(environment_id)
            .append(", 'Replica-set environment', 'replica-set-environment', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;

    let mut replica_set = workloads_support::exercise_replica_set(
        &executor,
        organization_id,
        project_id,
        environment_id,
    )
    .await?;
    let node_pool_id = fleet_support::exercise_fleet(&executor, organization_id).await?;
    workloads_support::exercise_workload_node_pool_selection(
        &executor,
        organization_id,
        project_id,
        environment_id,
        node_pool_id,
    )
    .await?;
    workloads_support::exercise_replica_evacuation(&executor, organization_id, &mut replica_set)
        .await?;
    workloads_support::exercise_replica_policy_v1_upgrade(
        &executor,
        organization_id,
        project_id,
        environment_id,
        &replica_set,
    )
    .await?;
    resource_claims_support::exercise_replica_anti_affinity(
        &executor,
        OrganizationId::from_uuid(organization_id),
        &replica_set,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_form_lifecycle_is_atomic_tenant_scoped_and_replay_safe() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, forms_support::exercise_form_persistence)
        .await
        .expect("PostgreSQL Form lifecycle gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_human_task_lifecycle_is_atomic_tenant_scoped_and_replay_safe() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(
        &admin_url,
        human_tasks_support::exercise_human_task_persistence,
    )
    .await
    .expect("PostgreSQL HumanTask lifecycle gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_human_task_flow_closes_the_authority_bound_decision_loop() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(
        &admin_url,
        human_tasks_support::exercise_human_task_flow_end_to_end,
    )
    .await
    .expect("PostgreSQL + A3S Flow HumanTask decision-loop gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identity_migration_backfills_legacy_credentials_and_ownerless_organizations() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_identity_migration_backfill)
        .await
        .expect("legacy Identity migration gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_hosted_draft_recovery_is_atomic() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_postgres_hosted_draft_recovery)
        .await
        .expect("hosted draft recovery PostgreSQL gate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_boot_flow_task_manager_drains_and_surfaces_terminal_failures() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_boot_flow_task_manager)
        .await
        .expect("Boot-backed Flow task manager PostgreSQL gate");
}

async fn run_isolated_postgres<F, Fut>(
    admin_url: &str,
    exercise: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    let isolated = IsolatedPostgresDatabase::create(admin_url).await?;
    let result = AssertUnwindSafe(Box::pin(exercise(isolated.url().to_owned())))
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

#[derive(Clone, Copy)]
struct ScheduledBootRuntime;

#[async_trait]
impl FlowRuntime for ScheduledBootRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.wait_completed("boot-due") {
            return Ok(context.complete(json!({"queue": "a3s-boot"})));
        }
        Ok(context.wait_until("boot-due", Utc::now() - chrono::Duration::seconds(1)))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<Value> {
        Err(FlowError::Runtime(format!(
            "scheduled Boot test does not execute step {}",
            invocation.step_name
        )))
    }
}

async fn exercise_boot_flow_task_manager(url: String) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
    let queue_options = QueueOptions::new()
        .with_poll_interval(Duration::from_millis(5))
        .with_lease_duration(Duration::from_millis(200));
    let flow = FlowInfrastructure::connect_with_queue_options(
        &url,
        Arc::new(ScheduledBootRuntime),
        queue_options,
    )
    .await?;
    let run_id = "cloud-boot-flow-success";
    flow.engine()
        .start_with_id(
            run_id,
            WorkflowSpec::rust_embedded("cloud.boot-flow-test", "1", "cloud", "run"),
            json!({}),
        )
        .await?;
    assert_eq!(
        flow.engine().snapshot(run_id).await?.status,
        WorkflowRunStatus::Suspended
    );
    let coordinator = boot_flow_coordinator(&executor, &flow)?;
    let report = coordinator.run_once().await?;
    assert_eq!(report.enqueued_tasks, 1);
    assert_eq!(report.handled_tasks, 1);
    let completed = flow.engine().snapshot(run_id).await?;
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output, Some(json!({"queue": "a3s-boot"})));
    assert!(flow.health().await.is_up());

    let database = Database::new(PostgresDialect, executor.clone());
    for relation in [
        "a3s_flow.a3s_orm_migrations",
        "a3s_flow.flow_events",
        "a3s_boot.a3s_orm_migrations",
        "a3s_boot.boot_queue_jobs",
    ] {
        let registered = database
            .fetch_one_as(
                sql_query::<Option<String>>("select to_regclass(")
                    .bind(relation)
                    .append(")::text"),
            )
            .await?;
        assert_eq!(registered.as_deref(), Some(relation));
    }

    let failing_run_id = "cloud-boot-flow-failure";
    flow.engine()
        .start_with_id(
            failing_run_id,
            WorkflowSpec::rust_embedded("cloud.boot-flow-failure", "1", "cloud", "run"),
            json!({}),
        )
        .await?;
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "create function reject_boot_flow_wait_completion() returns trigger language plpgsql as $$
               begin
                 if new.run_id = 'cloud-boot-flow-failure'
                    and new.event_json::jsonb ->> 'type' = 'wait_completed' then
                   raise exception 'injected Boot Flow terminal failure';
                 end if;
                 return new;
               end
             $$;
             create trigger reject_boot_flow_wait_completion
               before insert on a3s_flow.flow_events
               for each row execute function reject_boot_flow_wait_completion();",
        )
        .await?;
    let failure = boot_flow_coordinator(&executor, &flow)?
        .run_once()
        .await
        .expect_err("retry exhaustion must fail the coordinator cycle");
    assert!(matches!(
        &failure,
        a3s_cloud_control_plane::infrastructure::FlowCoordinatorError::TerminalTaskFailures {
            count: 1,
            ..
        }
    ));
    assert!(
        failure
            .to_string()
            .contains("A3S Flow task handling failed"),
        "unexpected terminal task failure: {failure}"
    );
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop trigger reject_boot_flow_wait_completion on a3s_flow.flow_events;
             drop function reject_boot_flow_wait_completion();",
        )
        .await?;
    let queue_states = database
        .fetch_one_as(sql_query::<(i64, i64, i64, i64)>(
            "select count(*) filter (where state = 'pending'), \
                    count(*) filter (where state = 'active'), \
                    count(*) filter (where state = 'failed'), \
                    coalesce(max(attempts_made) filter (where state = 'failed'), 0) \
             from a3s_boot.boot_queue_jobs \
             where queue_name = 'cloud-operations'",
        ))
        .await?;
    assert_eq!(queue_states, (0, 0, 1, 4));
    let health = flow.health().await;
    assert!(!health.is_up());
    assert_eq!(health.details["failedTasks"], 1);
    Ok(())
}

fn boot_flow_coordinator(
    executor: &PostgresExecutor,
    flow: &FlowInfrastructure,
) -> Result<FlowOperationCoordinator, Box<dyn std::error::Error>> {
    let operations: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operations,
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    Ok(FlowOperationCoordinator::new(
        reconciler,
        flow,
        Duration::from_millis(5),
        Duration::from_secs(2),
    )?)
}

async fn exercise_identity_migration_backfill(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = PostgresExecutor::connect_no_tls(&url, 2)?;
    executor
        .pool()
        .get()
        .await?
        .batch_execute(concat!(
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/001_foundation.sql"
            )),
            "\n",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/004_api_tokens.sql"
            ))
        ))
        .await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let platform_organization_id = Uuid::now_v7();
    let delegated_organization_id = Uuid::now_v7();
    let ownerless_organization_id = Uuid::now_v7();
    let created_at = Utc::now();
    for (id, name, name_key) in [
        (platform_organization_id, "Platform", "platform"),
        (delegated_organization_id, "Delegated", "delegated"),
        (ownerless_organization_id, "Ownerless", "ownerless"),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(id)
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(name_key)
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }

    let expired_platform_token_id = Uuid::now_v7();
    let platform_token_id = Uuid::now_v7();
    let delegated_owner_token_id = Uuid::now_v7();
    let delegated_admin_token_id = Uuid::now_v7();
    let delegated_member_token_id = Uuid::now_v7();
    for (
        id,
        organization_id,
        name,
        name_key,
        token_hash,
        scopes,
        token_created_at,
        expires_at,
        revoked_at,
    ) in [
        (
            expired_platform_token_id,
            platform_organization_id,
            "Expired platform administrator",
            "expired-platform-administrator",
            "legacy-expired-platform-token-digest",
            json!(["platform:write", "token:write"]),
            created_at - chrono::Duration::seconds(2),
            Some(created_at - chrono::Duration::seconds(1)),
            None,
        ),
        (
            platform_token_id,
            platform_organization_id,
            "Platform administrator",
            "platform-administrator",
            "legacy-platform-token-digest",
            json!(["platform:write", "token:write"]),
            created_at,
            None,
            None,
        ),
        (
            delegated_owner_token_id,
            delegated_organization_id,
            "Delegated owner",
            "delegated-owner",
            "legacy-delegated-owner-token-digest",
            json!(["token:write"]),
            created_at,
            None,
            Some(created_at + chrono::Duration::seconds(1)),
        ),
        (
            delegated_admin_token_id,
            delegated_organization_id,
            "Delegated administrator",
            "delegated-administrator",
            "legacy-delegated-administrator-token-digest",
            json!(["token:write"]),
            created_at + chrono::Duration::seconds(2),
            None,
            None,
        ),
        (
            delegated_member_token_id,
            delegated_organization_id,
            "Delegated member",
            "delegated-member",
            "legacy-delegated-member-token-digest",
            json!(["cloud:read"]),
            created_at + chrono::Duration::seconds(3),
            None,
            None,
        ),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into api_tokens (id, organization_id, name, name_key, token_hash, scopes, aggregate_version, created_at, expires_at, revoked_at) values (",
                )
                .bind(id)
                .append(", ")
                .bind(organization_id)
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(name_key)
                .append(", ")
                .bind(token_hash)
                .append(", ")
                .bind(scopes)
                .append(", 1, ")
                .bind(token_created_at)
                .append(", ")
                .bind(expires_at)
                .append(", ")
                .bind(revoked_at)
                .append(")"),
            )
            .await?;
    }

    Migrator::new(executor.clone())
        .run([Migration::new(
            "074",
            "Identity principals and organization memberships",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/074_identity_principals_and_memberships.sql"
            )),
        )])
        .await?;

    let authority_counts = database
        .fetch_one_as(sql_query::<(i64, i64, i64)>(
            "select (select count(*) from identity_principals), (select count(*) from organization_memberships), (select count(*) from api_tokens where principal_id = id)",
        ))
        .await?;
    assert_eq!(authority_counts, (5, 6, 5));
    let delegated_roles = database
        .fetch_one_as(
            sql_query::<(String, String, String)>(
                "select (select role from organization_memberships where principal_id = ",
            )
            .bind(delegated_owner_token_id)
            .append("), (select role from organization_memberships where principal_id = ")
            .bind(delegated_admin_token_id)
            .append("), (select role from organization_memberships where principal_id = ")
            .bind(delegated_member_token_id)
            .append(")"),
        )
        .await?;
    assert_eq!(
        delegated_roles,
        ("admin".into(), "owner".into(), "member".into())
    );
    let ownerless_owner = database
        .fetch_one_as(
            sql_query::<Uuid>(
                "select principal_id from organization_memberships where organization_id = ",
            )
            .bind(ownerless_organization_id)
            .append(" and role = 'owner' and revoked_at is null"),
        )
        .await?;
    assert_eq!(ownerless_owner, platform_token_id);
    let scope_counts = database
        .fetch_one_as(sql_query::<(i64, i64)>(
            "select count(*) filter (where scopes ? 'identity:write'), count(*) filter (where not scopes ? 'identity:write') from api_tokens",
        ))
        .await?;
    assert_eq!(scope_counts, (4, 1));
    let organizations_without_owner = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from organizations organization where not exists (select 1 from organization_memberships membership where membership.organization_id = organization.id and membership.role = 'owner' and membership.revoked_at is null)",
        ))
        .await?;
    assert_eq!(organizations_without_owner, 0);
    Ok(())
}

async fn exercise_postgres_hosted_draft_recovery(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Hosted recovery tenant', 'hosted-recovery-tenant', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("Hosted recovery Agent")?,
        AssetKind::Agent,
        created_at,
    )?;
    PostgresAssetRepository::new(executor.clone())
        .create_asset(CreateAssetWrite {
            event: AssetCreated::envelope(&asset, asset.id.as_uuid())?,
            idempotency: IdempotencyRequest::new(
                format!("organizations/{organization_id}/assets"),
                "postgres-hosted-recovery-asset",
                asset.id.as_uuid().as_bytes(),
            )?,
            asset: asset.clone(),
        })
        .await?;

    build_runs_support::exercise_hosted_build_run_persistence(&executor, &asset).await
}

async fn exercise_postgres_foundation(url: String) -> Result<(), Box<dyn std::error::Error>> {
    let admin = PostgresExecutor::connect_no_tls(&url, 4)?;
    admin
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop schema if exists a3s_flow cascade;
             drop schema if exists a3s_boot cascade;
             drop table if exists resource_slot_leases cascade;
             drop table if exists resource_claim_slots cascade;
             drop table if exists resource_claims cascade;
             drop table if exists github_connection_lifecycle_inbox cascade;
             drop table if exists github_repository_subscriptions cascade;
             drop table if exists github_source_connections cascade;
             drop table if exists github_connection_flows cascade;
             drop table if exists source_webhook_inbox cascade;
             drop table if exists source_webhook_deliveries cascade;
             drop table if exists build_runs cascade;
             drop table if exists external_source_revisions cascade;
             drop table if exists secret_rotation_reconciliations cascade;
             drop table if exists secret_rotation_restarts cascade;
             drop table if exists secret_versions cascade;
             drop table if exists secrets cascade;
             drop table if exists gateway_certificate_convergences cascade;
             drop table if exists gateway_route_cutovers cascade;
             drop table if exists mcp_gateway_snapshot_publications cascade;
             drop table if exists gateway_route_ownership cascade;
             drop table if exists gateway_rollout_rollbacks cascade;
             drop table if exists gateway_route_projections cascade;
             drop table if exists gateway_rollout_replicas cascade;
             drop table if exists gateway_rollouts cascade;
             drop table if exists deployments cascade;
             drop table if exists workload_revisions cascade;
             drop table if exists workloads cascade;
             drop table if exists agent_execution_events cascade;
             drop table if exists agent_executions cascade;
             drop table if exists agent_conversations cascade;
             drop table if exists executions cascade;
             drop table if exists routes cascade;
             drop table if exists gateway_route_scopes cascade;
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
             drop table if exists node_resource_inventory_heads cascade;
             drop table if exists node_resource_inventory_slots cascade;
             drop table if exists node_resource_inventories cascade;
             drop table if exists node_commands cascade;
             drop table if exists node_certificate_rotations cascade;
             drop table if exists node_certificates cascade;
             drop table if exists node_enrollment_reservations cascade;
             drop table if exists nodes cascade;
             drop table if exists enrollment_tokens cascade;
             drop table if exists plugin_registries cascade;
             drop table if exists organization_memberships cascade;
             drop table if exists api_tokens cascade;
             drop table if exists identity_principals cascade;
             drop table if exists workflow_resume_receipts cascade;
             drop table if exists workflow_resume_outbox cascade;
             drop table if exists workflow_human_task_inbox cascade;
             drop table if exists workflow_decisions cascade;
             drop table if exists human_tasks cascade;
             drop table if exists form_submissions cascade;
             drop table if exists form_releases cascade;
             drop table if exists form_drafts cascade;
             drop table if exists ontology_revisions cascade;
             drop table if exists ontologies cascade;
             drop table if exists operation_projections cascade;
             drop table if exists operation_requests cascade;
             drop table if exists audit_records cascade;
             drop table if exists outbox_events cascade;
             drop table if exists idempotency_records cascade;
             drop table if exists mcp_credentials cascade;
             drop table if exists mcp_route_policies cascade;
             drop table if exists mcp_service_profiles cascade;
             drop table if exists asset_git_repository_controls cascade;
             drop table if exists asset_releases cascade;
             drop table if exists assets cascade;
             drop table if exists environments cascade;
             drop table if exists projects cascade;
             drop table if exists organizations cascade;
             drop table if exists a3s_orm_rollback_probe cascade;
             drop table if exists a3s_orm_migrations cascade;
             drop function if exists reject_cloud_outbox() cascade;
             drop function if exists reject_outbox_ack() cascade;
             drop function if exists reject_human_task_immutable_mutation() cascade;
             drop function if exists protect_human_task_authority() cascade;
             drop function if exists protect_workflow_resume_outbox_authority() cascade;
             drop function if exists reject_form_release_mutation() cascade;
             drop function if exists reject_ontology_revision_mutation() cascade;",
        )
        .await?;

    let (left, right) = tokio::join!(connect_and_migrate(&url, 4), connect_and_migrate(&url, 4));
    let executor = left?;
    right?;
    let database = Database::new(PostgresDialect, executor.clone());
    let applied = database
        .fetch_one_as(sql_query::<i64>("select count(*) from a3s_orm_migrations"))
        .await?;
    assert_eq!(applied, 90);
    let boot_schema = database
        .fetch_one_as(sql_query::<Option<String>>(
            "select to_regnamespace('a3s_boot')::text",
        ))
        .await?;
    assert_eq!(boot_schema.as_deref(), Some("a3s_boot"));
    for table in [
        "agent_conversations",
        "agent_executions",
        "agent_execution_events",
        "form_drafts",
        "form_releases",
        "form_submissions",
        "human_tasks",
        "ontologies",
        "ontology_revisions",
        "plugin_registries",
        "workflow_decisions",
        "workflow_human_task_inbox",
        "workflow_resume_outbox",
        "workflow_resume_receipts",
        "workflow_runs",
        "workflow_step_projections",
    ] {
        let relation = database
            .fetch_one_as(
                sql_query::<Option<String>>("select to_regclass(")
                    .bind(format!("public.{table}"))
                    .append(")::text"),
            )
            .await?;
        assert_eq!(relation.as_deref(), Some(table));
    }
    let workflow_run_trigger_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_trigger where tgrelid in ('workflow_runs'::regclass, 'workflow_step_projections'::regclass) and not tgisinternal",
        ))
        .await?;
    assert_eq!(workflow_run_trigger_count, 4);
    let immutable_human_record_trigger_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_trigger where tgrelid in ('form_submissions'::regclass, 'workflow_decisions'::regclass, 'workflow_human_task_inbox'::regclass, 'workflow_resume_receipts'::regclass) and not tgisinternal",
        ))
        .await?;
    assert_eq!(immutable_human_record_trigger_count, 4);
    let protected_human_task_trigger_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_trigger where tgrelid in ('human_tasks'::regclass, 'workflow_resume_outbox'::regclass) and not tgisinternal",
        ))
        .await?;
    assert_eq!(protected_human_task_trigger_count, 4);
    for index in [
        "workflow_runs_project_requested_idx",
        "workflow_runs_reconciliation_idx",
        "workflow_step_projections_run_status_idx",
        "human_tasks_project_status_idx",
        "human_tasks_run_step_idx",
        "workflow_resume_outbox_delivery_idx",
        "workflow_human_task_inbox_observed_idx",
    ] {
        let relation = database
            .fetch_one_as(
                sql_query::<Option<String>>("select to_regclass(")
                    .bind(format!("public.{index}"))
                    .append(")::text"),
            )
            .await?;
        assert_eq!(relation.as_deref(), Some(index));
    }
    let workflow_step_kind_constraint = database
        .fetch_one_as(sql_query::<String>(
            "select pg_get_constraintdef(oid) from pg_constraint where conrelid = 'workflow_step_projections'::regclass and conname = 'workflow_step_projections_kind_check'",
        ))
        .await?;
    assert!(workflow_step_kind_constraint.contains("'human_decision'"));
    let decision_submission_foreign_key = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conrelid = 'workflow_decisions'::regclass and conname = 'workflow_decisions_submission_fk'",
        ))
        .await?;
    assert_eq!(decision_submission_foreign_key, 1);
    let node_command_kind_constraint = database
        .fetch_one_as(sql_query::<String>(
            "select pg_get_constraintdef(oid) from pg_constraint where conrelid = 'node_commands'::regclass and conname = 'node_commands_command_kind_check'",
        ))
        .await?;
    for kind in [
        "plugin_host_capabilities_inspect",
        "plugin_host_plan",
        "plugin_host_apply",
        "plugin_host_plan_enablement",
        "plugin_host_observe",
    ] {
        assert!(
            node_command_kind_constraint.contains(&format!("'{kind}'")),
            "node command kind constraint omitted {kind}"
        );
    }
    assert!(!node_command_kind_constraint.contains("plugin_host_execute"));
    assert!(!node_command_kind_constraint.contains("plugin_host_set_enablement"));
    let search_projection = database
        .fetch_one_as(sql_query::<Option<String>>(
            "select to_regclass('public.authorized_search_projections')::text",
        ))
        .await?;
    assert_eq!(
        search_projection.as_deref(),
        Some("authorized_search_projections")
    );
    let search_projection_definition = database
        .fetch_one_as(sql_query::<String>(
            "select pg_get_viewdef('authorized_search_projections'::regclass, true)",
        ))
        .await?;
    assert!(search_projection_definition.contains("'ontology'::text"));
    assert!(search_projection_definition.contains("'plugin_registry'::text"));
    assert!(search_projection_definition.contains("plugin_registries"));
    let immutable_revision_trigger = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_trigger where tgrelid = 'ontology_revisions'::regclass and tgname = 'ontology_revisions_immutable' and not tgisinternal",
        ))
        .await?;
    assert_eq!(immutable_revision_trigger, 1);
    let immutable_form_release_trigger = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_trigger where tgrelid = 'form_releases'::regclass and tgname = 'form_releases_immutable' and not tgisinternal",
        ))
        .await?;
    assert_eq!(immutable_form_release_trigger, 1);
    let latest_form_release_foreign_key = database
        .fetch_one_as(sql_query::<(bool, bool)>(
            "select condeferrable, condeferred from pg_constraint where conrelid = 'form_drafts'::regclass and conname = 'form_drafts_latest_release_fk'",
        ))
        .await?;
    assert_eq!(latest_form_release_foreign_key, (true, true));
    let form_release_uniqueness = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conrelid = 'form_releases'::regclass and conname in ('form_releases_revision_unique', 'form_releases_source_draft_version_unique')",
        ))
        .await?;
    assert_eq!(form_release_uniqueness, 2);
    assert_route_target_migration_backfills_legacy_projection(&executor).await?;
    assert_logical_gateway_scope_migration_backfills_legacy_projection(&executor).await?;
    assert_gateway_management_protocol_migration_preserves_legacy_acknowledgements(&executor)
        .await?;
    assert_gateway_scope_membership_migration_backfills_primary_members(&executor).await?;
    assert_box_native_build_authority_migration_invalidates_legacy_runs(&executor).await?;
    let evidence_required_column = database
        .fetch_one_as(sql_query::<(String, String, Option<String>)>(
            "select data_type, is_nullable, column_default from information_schema.columns where table_schema = 'public' and table_name = 'build_runs' and column_name = 'evidence_required'",
        ))
        .await?;
    assert_eq!(
        evidence_required_column,
        ("boolean".into(), "NO".into(), None)
    );
    let evidence_column = database
        .fetch_one_as(sql_query::<(String, String, Option<String>)>(
            "select data_type, is_nullable, column_default from information_schema.columns where table_schema = 'public' and table_name = 'build_runs' and column_name = 'evidence'",
        ))
        .await?;
    assert_eq!(evidence_column, ("jsonb".into(), "YES".into(), None));
    let retired_build_columns = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from information_schema.columns where table_schema = 'public' and table_name = 'build_runs' and column_name in ('runtime_spec_digest', 'runtime_output_artifact', 'cache_required', 'cache')",
        ))
        .await?;
    assert_eq!(retired_build_columns, 0);
    let box_build_columns = database
        .fetch_all_as(sql_query::<(String, String)>(
            "select column_name, data_type from information_schema.columns where table_schema = 'public' and table_name = 'build_runs' and column_name in ('build_request_digest', 'box_build_output') order by column_name",
        ))
        .await?;
    assert_eq!(
        box_build_columns.rows,
        vec![
            ("box_build_output".into(), "jsonb".into()),
            ("build_request_digest".into(), "text".into()),
        ]
    );
    let build_subject_columns = database
        .fetch_all_as(sql_query::<(String, String, String, Option<String>)>(
            "select column_name, data_type, is_nullable, column_default from information_schema.columns where table_schema = 'public' and table_name = 'build_runs' and column_name in ('subject_kind', 'project_id', 'environment_id', 'source_revision_id', 'asset_id', 'asset_release_id') order by column_name",
        ))
        .await?;
    assert_eq!(
        build_subject_columns.rows,
        vec![
            ("asset_id".into(), "uuid".into(), "YES".into(), None),
            ("asset_release_id".into(), "uuid".into(), "YES".into(), None,),
            ("environment_id".into(), "uuid".into(), "YES".into(), None),
            ("project_id".into(), "uuid".into(), "YES".into(), None),
            (
                "source_revision_id".into(),
                "uuid".into(),
                "YES".into(),
                None,
            ),
            ("subject_kind".into(), "text".into(), "NO".into(), None),
        ]
    );
    let build_subject_constraints = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conrelid = 'build_runs'::regclass and conname in ('build_runs_subject_shape_check', 'build_runs_asset_release_foreign_key')",
        ))
        .await?;
    assert_eq!(build_subject_constraints, 2);
    let build_subject_indexes = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_indexes where schemaname = 'public' and tablename = 'build_runs' and indexname in ('build_runs_external_subject_attempt_unique', 'build_runs_asset_release_attempt_unique')",
        ))
        .await?;
    assert_eq!(build_subject_indexes, 2);
    let release_provenance_columns = database
        .fetch_all_as(sql_query::<(String, String, String, Option<String>)>(
            "select column_name, data_type, is_nullable, column_default from information_schema.columns where table_schema = 'public' and table_name = 'asset_releases' and column_name in ('build_run_id', 'provenance_digest') order by column_name",
        ))
        .await?;
    assert_eq!(
        release_provenance_columns.rows,
        vec![
            ("build_run_id".into(), "uuid".into(), "YES".into(), None),
            (
                "provenance_digest".into(),
                "text".into(),
                "YES".into(),
                None,
            ),
        ]
    );
    let release_provenance_constraints = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conname in ('build_runs_hosted_release_publication_identity_unique', 'asset_releases_provenance_digest_check', 'asset_releases_publication_provenance_shape_check', 'asset_releases_hosted_build_foreign_key')",
        ))
        .await?;
    assert_eq!(release_provenance_constraints, 4);
    let box_build_constraint_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conrelid = 'build_runs'::regclass and conname in ('build_runs_box_chain_check', 'build_runs_box_output_shape_check', 'build_runs_validated_output_check')",
        ))
        .await?;
    assert_eq!(box_build_constraint_count, 3);
    let build_evidence_constraint_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from pg_constraint where conrelid = 'build_runs'::regclass and conname in ('build_runs_status_check', 'build_runs_evidence_shape_check', 'build_runs_execution_state_check', 'build_runs_required_evidence_cleanup_check', 'build_runs_success_check', 'build_runs_cancelled_check')",
        ))
        .await?;
    assert_eq!(build_evidence_constraint_count, 6);
    let build_status_constraint = database
        .fetch_one_as(sql_query::<String>(
            "select pg_get_constraintdef(oid) from pg_constraint where conrelid = 'build_runs'::regclass and conname = 'build_runs_status_check'",
        ))
        .await?;
    assert!(build_status_constraint.contains("'attesting'"));
    let build_publication_target_constraint = database
        .fetch_one_as(sql_query::<String>(
            "select pg_get_constraintdef(oid) from pg_constraint where conrelid = 'build_runs'::regclass and conname = 'build_runs_publication_target_check'",
        ))
        .await?;
    assert!(build_publication_target_constraint.contains("'attesting'"));
    let build_evidence_constraint = database
        .fetch_one_as(sql_query::<String>(
            "select pg_get_constraintdef(oid) from pg_constraint where conrelid = 'build_runs'::regclass and conname = 'build_runs_evidence_shape_check'",
        ))
        .await?;
    assert!(build_evidence_constraint.contains("jsonb_typeof"));
    assert!(build_evidence_constraint.contains("verificationState"));
    assert!(build_evidence_constraint.contains("ed25519"));
    assert!(build_evidence_constraint.contains("publicKey"));
    assert!(build_evidence_constraint.contains("assetReleaseId"));
    assert!(build_evidence_constraint.contains("manifestDigest"));
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
                "durable source build runs",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/026_build_runs.sql"
                )),
            ),
            Migration::new(
                "027",
                "durable OCI build publications",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/027_build_publications.sql"
                )),
            ),
            Migration::new(
                "028",
                "external build workload handoff",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/028_external_build_workload_handoff.sql"
                )),
            ),
            Migration::new(
                "029",
                "GitHub provider authority",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/029_github_provider_authority.sql"
                )),
            ),
            Migration::new(
                "030",
                "build run attempts",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/030_build_run_attempts.sql"
                )),
            ),
            Migration::new(
                "031",
                "verified build evidence",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/031_build_evidence.sql"
                )),
            ),
            Migration::new(
                "032",
                "trusted build cache",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/032_build_cache_trust.sql"
                )),
            ),
            Migration::new(
                "033",
                "managed Gateway snapshot validity",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/033_gateway_snapshot_validity.sql"
                )),
            ),
            Migration::new(
                "034",
                "managed Gateway snapshot renewal",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/034_gateway_snapshot_renewal.sql"
                )),
            ),
            Migration::new(
                "035",
                "generation-bound Gateway route targets",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/035_route_target_generation.sql"
                )),
            ),
            Migration::new(
                "036",
                "logical Gateway scopes",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/036_logical_gateway_scopes.sql"
                )),
            ),
            Migration::new(
                "037",
                "Gateway management protocol evidence",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/037_gateway_management_protocol.sql"
                )),
            ),
            Migration::new(
                "038",
                "replicated Gateway scope membership",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/038_gateway_scope_membership.sql"
                )),
            ),
            Migration::new(
                "039",
                "per-replica Gateway rollouts",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/039_gateway_replica_rollouts.sql"
                )),
            ),
            Migration::new(
                "040",
                "managed Workload replica foundation",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/040_workload_replica_foundation.sql"
                )),
            ),
            Migration::new(
                "041",
                "fenced hard resource claims",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/041_hard_resource_claims.sql"
                )),
            ),
            Migration::new(
                "042",
                "node resource inventories",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/042_node_resource_inventories.sql"
                )),
            ),
            Migration::new(
                "043",
                "shared resource capacity accounting",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/043_shared_resource_capacity.sql"
                )),
            ),
            Migration::new(
                "044",
                "Agent resource Claim commands",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/044_resource_claim_commands.sql"
                )),
            ),
            Migration::new(
                "045",
                "Gateway Route rollout projections",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/045_gateway_route_rollout_projections.sql"
                )),
            ),
            Migration::new(
                "046",
                "Gateway snapshot observation commands",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/046_gateway_snapshot_observation_commands.sql"
                )),
            ),
            Migration::new(
                "047",
                "Gateway replica physical-state recovery",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/047_gateway_replica_recovery.sql"
                )),
            ),
            Migration::new(
                "048",
                "Gateway rollout exact rollback",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/048_gateway_rollout_rollbacks.sql"
                )),
            ),
            Migration::new(
                "049",
                "Gateway certificate convergence unavailability",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/049_gateway_certificate_convergence_unavailable.sql"
                )),
            ),
            Migration::new(
                "050",
                "tenant-authorized search projections",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/050_authorized_search_projections.sql"
                )),
            ),
            Migration::new(
                "051",
                "hosted Asset and immutable release foundation",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/051_hosted_assets.sql"
                )),
            ),
            Migration::new(
                "052",
                "Cloud executions",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/052_executions.sql"
                )),
            ),
            Migration::new(
                "053",
                "immutable hosted MCP Service profiles",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/053_mcp_service_profiles.sql"
                )),
            ),
            Migration::new(
                "054",
                "mutable hosted MCP route policies",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/054_mcp_route_policies.sql"
                )),
            ),
            Migration::new(
                "055",
                "hosted MCP Workload revision release bindings",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/055_mcp_workload_revision_bindings.sql"
                )),
            ),
            Migration::new(
                "056",
                "hosted MCP credential authority",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/056_mcp_credentials.sql"
                )),
            ),
            Migration::new(
                "057",
                "hosted MCP Gateway snapshot publication identity",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/057_mcp_gateway_snapshot_publications.sql"
                )),
            ),
            Migration::new(
                "058",
                "hosted MCP Gateway desired-state identity",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/058_mcp_gateway_desired_state.sql"
                )),
            ),
            Migration::new(
                "059",
                "Box native build node commands",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/059_box_build_commands.sql"
                )),
            ),
            Migration::new(
                "060",
                "sole Box native build authority",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/060_box_native_build_authority.sql"
                )),
            ),
            Migration::new(
                "061",
                "hosted Asset Git repository controls",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/061_asset_git_repository_controls.sql"
                )),
            ),
            Migration::new(
                "062",
                "canonical A3S Runtime artifact JSON contract",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/062_runtime_artifact_json_contract.sql"
                )),
            ),
            Migration::new(
                "063",
                "hosted Asset build run subjects",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/063_hosted_asset_build_runs.sql"
                )),
            ),
            Migration::new(
                "064",
                "atomic hosted Asset release publication",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/064_atomic_hosted_release_publication.sql"
                )),
            ),
            Migration::new(
                "065",
                "A3S Use Plugin Host node commands",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/065_plugin_host_commands.sql"
                )),
            ),
            Migration::new(
                "066",
                "Agent workload release bindings",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/066_agent_workload_release_bindings.sql"
                )),
            ),
            Migration::new(
                "067",
                "Skill workload revision bindings",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/067_skill_workload_revision_bindings.sql"
                )),
            ),
            Migration::new(
                "068",
                "Agent conversations and executions",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/068_agent_conversations_and_executions.sql"
                )),
            ),
            Migration::new(
                "069",
                "Agent A3S Code run bindings",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/069_agent_code_run_bindings.sql"
                )),
            ),
            Migration::new(
                "070",
                "Agent execution cancellation",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../migrations/070_agent_execution_cancellation.sql"
                )),
            ),
            Migration::new(
                "071",
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
    let asset_repository_directory = security_directory.path().join("asset-repositories");
    application_config.assets.repository_dir = asset_repository_directory.display().to_string();
    application_config.artifacts.store_dir = security_directory
        .path()
        .join("immutable-objects")
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

    let memberships_path = format!("/api/v1/organizations/{organization_id}/memberships");
    let initial_memberships = app.call(get_as(&memberships_path, ADMIN_TOKEN)).await?;
    assert_eq!(initial_memberships.status(), 200);
    let owner_membership_id = response_json(&initial_memberships)?["data"][0]["id"]
        .as_str()
        .ok_or("bootstrap membership has no ID")?
        .to_owned();
    let owner_principal_id = response_json(&initial_memberships)?["data"][0]["principalId"]
        .as_str()
        .ok_or("bootstrap membership has no principal ID")?
        .to_owned();

    let plugin_registry_id = Uuid::now_v7();
    let plugin_registry_request_id = Uuid::now_v7();
    let plugin_registry_created_at = Utc::now();
    let plugin_root_hex = "a".repeat(64);
    database
        .execute(
            sql_query::<()>(
                "insert into plugin_registries (organization_id, id, name, name_key, endpoint, root_object_ref, root_sha256, root_version, state, aggregate_version, last_actor_id, last_request_id, created_at, updated_at) values (",
            )
            .bind(Uuid::parse_str(&organization_id)?)
            .append(", ")
            .bind(plugin_registry_id)
            .append(", ")
            .bind("Official plugins")
            .append(", ")
            .bind("official plugins")
            .append(", ")
            .bind("https://registry.example/plugins/")
            .append(", ")
            .bind(format!("sha256/{plugin_root_hex}/root.json"))
            .append(", ")
            .bind(format!("sha256:{plugin_root_hex}"))
            .append(", 1, ")
            .bind("active")
            .append(", 1, ")
            .bind(Uuid::parse_str(&owner_principal_id)?)
            .append(", ")
            .bind(plugin_registry_request_id)
            .append(", ")
            .bind(plugin_registry_created_at)
            .append(", ")
            .bind(plugin_registry_created_at)
            .append(")"),
        )
        .await?;

    let plugin_registry_search = app
        .call(get_as(
            format!("/api/v1/organizations/{organization_id}/search?q=official&limit=20"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(plugin_registry_search.status(), 200);
    let plugin_registry_search_body = response_json(&plugin_registry_search)?;
    assert_eq!(
        plugin_registry_search_body["data"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        plugin_registry_search_body["data"][0]["kind"],
        "plugin_registry"
    );
    assert_eq!(
        plugin_registry_search_body["data"][0]["id"],
        plugin_registry_id.to_string()
    );
    assert_eq!(
        plugin_registry_search_body["data"][0]["href"],
        format!("#/organizations/{organization_id}/plugin-registries/{plugin_registry_id}")
    );
    assert_eq!(
        plugin_registry_search_body["data"][0]["description"],
        "Plugin registry · https://registry.example/plugins/"
    );
    assert!(!plugin_registry_search_body
        .to_string()
        .contains(&plugin_root_hex));

    let service_membership = app
        .call(post_json(
            &memberships_path,
            "membership-service-automation",
            json!({"name": "service automation", "role": "member"}),
        ))
        .await?;
    let service_membership_replay = app
        .call(post_json(
            &memberships_path,
            "membership-service-automation",
            json!({"name": "service automation", "role": "member"}),
        ))
        .await?;
    assert_eq!(service_membership.status(), 201);
    assert_eq!(service_membership_replay.status(), 200);
    let service_membership_data = response_json(&service_membership)?["data"].clone();
    let service_membership_id = service_membership_data["id"]
        .as_str()
        .ok_or("service membership has no ID")?
        .to_owned();
    let service_principal_id = service_membership_data["principalId"]
        .as_str()
        .ok_or("service membership has no principal ID")?
        .to_owned();
    assert_eq!(
        response_json(&service_membership_replay)?["data"]["id"],
        service_membership_id
    );
    let stored_membership_authority = database
        .fetch_one_as(
            sql_query::<(i64, i64)>(
                "select (select count(*) from identity_principals), (select count(*) from organization_memberships where organization_id = ",
            )
            .bind(Uuid::parse_str(&organization_id)?)
            .append(")"),
        )
        .await?;
    assert_eq!(stored_membership_authority, (2, 2));

    let service_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/api-tokens"),
            "membership-service-token",
            json!({
                "name": "service automation",
                "token": SERVICE_MEMBER_TOKEN,
                "scopes": ["project:write", "identity:write", "token:write"],
                "principalId": service_principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(service_token.status(), 201);
    assert_eq!(
        response_json(&service_token)?["data"]["principalId"],
        service_principal_id
    );
    let privilege_escalation = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization_id}/api-tokens"),
            "membership-privilege-escalation",
            json!({
                "name": "forged owner credential",
                "token": PRIVILEGE_ESCALATION_TOKEN,
                "scopes": ["cloud:read"],
                "principalId": owner_principal_id,
                "expiresAt": null
            }),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(privilege_escalation.status(), 403);

    let project_path = format!("/api/v1/organizations/{organization_id}/projects");
    let service_role_path = format!("{memberships_path}/{service_membership_id}/role");
    let promoted = app
        .call(post_json(
            &service_role_path,
            "membership-service-promote-admin",
            json!({"role": "admin", "expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(promoted.status(), 200);
    let admin_privilege_escalation = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization_id}/api-tokens"),
            "membership-admin-privilege-escalation",
            json!({
                "name": "forged owner credential",
                "token": PRIVILEGE_ESCALATION_TOKEN,
                "scopes": ["cloud:read"],
                "principalId": owner_principal_id,
                "expiresAt": null
            }),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(admin_privilege_escalation.status(), 403);
    let returned_to_member = app
        .call(post_json(
            &service_role_path,
            "membership-service-return-to-member",
            json!({"role": "member", "expectedVersion": 2}),
        ))
        .await?;
    assert_eq!(returned_to_member.status(), 200);

    let service_project = app
        .call(post_json_as(
            &project_path,
            "membership-service-project",
            json!({"name": "Service Project"}),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(service_project.status(), 201);
    let service_project_id = response_id(&service_project)?;
    let restricted = app
        .call(post_json(
            &service_role_path,
            "membership-service-restrict",
            json!({"role": "restricted", "expectedVersion": 3}),
        ))
        .await?;
    assert_eq!(restricted.status(), 200);
    assert_eq!(
        app.call(get_as(&project_path, SERVICE_MEMBER_TOKEN))
            .await?
            .status(),
        403
    );
    let resource_grants_path = format!(
        "/api/v1/organizations/{organization_id}/memberships/{service_membership_id}/resource-grants"
    );
    let missing_target = app
        .call(post_json(
            &resource_grants_path,
            "resource-grant-missing-project",
            json!({
                "scope": {"kind": "project", "projectId": Uuid::now_v7()}
            }),
        ))
        .await?;
    assert_eq!(missing_target.status(), 404);
    let grant_body = json!({
        "scope": {"kind": "project", "projectId": service_project_id}
    });
    let resource_grant = app
        .call(post_json(
            &resource_grants_path,
            "resource-grant-service-project",
            grant_body.clone(),
        ))
        .await?;
    let resource_grant_replay = app
        .call(post_json(
            &resource_grants_path,
            "resource-grant-service-project",
            grant_body,
        ))
        .await?;
    assert_eq!(resource_grant.status(), 201);
    assert_eq!(resource_grant_replay.status(), 200);
    let resource_grant_id = response_id(&resource_grant)?;
    let resource_grant_uuid = Uuid::parse_str(&resource_grant_id)?;
    assert_eq!(response_id(&resource_grant_replay)?, resource_grant_id);
    assert_eq!(
        response_json(&resource_grant_replay)?["data"]["replayed"],
        true
    );

    let listed_grants = app.call(get_as(&resource_grants_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed_grants.status(), 200);
    assert_eq!(
        response_json(&listed_grants)?["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let resource_grant_path =
        format!("/api/v1/organizations/{organization_id}/resource-grants/{resource_grant_id}");
    assert_eq!(
        app.call(get_as(&resource_grant_path, ADMIN_TOKEN))
            .await?
            .status(),
        200
    );
    let visible_projects = app
        .call(get_as(&project_path, SERVICE_MEMBER_TOKEN))
        .await?;
    assert_eq!(visible_projects.status(), 200);
    assert_eq!(
        response_json(&visible_projects)?["data"][0]["id"],
        service_project_id.to_string()
    );

    let revoke_arguments = json!({"expectedVersion": 1});
    let revoked_grant = app
        .call(post_json(
            format!("{resource_grant_path}/revocation"),
            "resource-grant-service-project-revoke",
            revoke_arguments.clone(),
        ))
        .await?;
    let revoked_grant_replay = app
        .call(post_json(
            format!("{resource_grant_path}/revocation"),
            "resource-grant-service-project-revoke",
            revoke_arguments,
        ))
        .await?;
    assert_eq!(revoked_grant.status(), 200);
    assert_eq!(revoked_grant_replay.status(), 200);
    assert_eq!(
        response_json(&revoked_grant)?["data"]["aggregateVersion"],
        2
    );
    assert!(response_json(&revoked_grant)?["data"]["revokedAt"].is_string());
    assert_eq!(
        response_json(&revoked_grant_replay)?["data"]["replayed"],
        true
    );
    assert_eq!(
        app.call(get_as(&project_path, SERVICE_MEMBER_TOKEN))
            .await?
            .status(),
        403
    );
    let stored_resource_grant_evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>(
                "select (select count(*) from resource_grants where id = ",
            )
            .bind(resource_grant_uuid)
            .append(" and aggregate_version = 2 and revoked_at is not null), (select count(*) from audit_records where aggregate_id = ")
            .bind(resource_grant_uuid)
            .append(" and action like 'identity.resource-grant.%'), (select count(*) from outbox_events where aggregate_id = ")
            .bind(resource_grant_uuid)
            .append(" and event_key like 'identity.resource-grant.%')"),
        )
        .await?;
    assert_eq!(stored_resource_grant_evidence, (1, 2, 2));
    let restored = app
        .call(post_json(
            &service_role_path,
            "membership-service-restore",
            json!({"role": "member", "expectedVersion": 4}),
        ))
        .await?;
    assert_eq!(restored.status(), 200);
    assert_eq!(
        app.call(get_as(&project_path, SERVICE_MEMBER_TOKEN))
            .await?
            .status(),
        200
    );
    let revoked_membership = app
        .call(post_json(
            format!("{memberships_path}/{service_membership_id}/revocation"),
            "membership-service-revoke",
            json!({"expectedVersion": 5}),
        ))
        .await?;
    assert_eq!(revoked_membership.status(), 200);
    assert_eq!(
        app.call(get_as(&project_path, SERVICE_MEMBER_TOKEN))
            .await?
            .status(),
        401
    );
    let last_owner = app
        .call(post_json(
            format!("{memberships_path}/{owner_membership_id}/revocation"),
            "membership-last-owner",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(last_owner.status(), 409);
    let membership_audits = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from audit_records where aggregate_id = ")
                .bind(Uuid::parse_str(&service_membership_id)?)
                .append(" and action like 'identity.membership.%'"),
        )
        .await?;
    assert_eq!(membership_audits, 6);

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

    let ontology_path =
        format!("/api/v1/organizations/{organization_id}/projects/{project_id}/ontologies");
    let create_ontology = || {
        BootRequest::new(HttpMethod::Post, &ontology_path)
            .with_header("content-type", "application/vnd.a3s.acl")
            .with_header("idempotency-key", "ontology-support")
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
            .with_body(ONTOLOGY_ACL.as_bytes().to_vec())
    };
    let ontology = app.call(create_ontology()).await?;
    let ontology_replay = app.call(create_ontology()).await?;
    assert_eq!(ontology.status(), 201);
    assert_eq!(ontology_replay.status(), 200);
    let ontology_body = response_json(&ontology)?;
    let ontology_id = ontology_body["data"]["ontology"]["id"]
        .as_str()
        .ok_or("Ontology response has no ID")?
        .to_owned();
    assert_eq!(
        response_json(&ontology_replay)?["data"]["ontology"]["id"],
        ontology_id
    );
    let ontology_root = format!("/api/v1/organizations/{organization_id}/ontologies/{ontology_id}");
    let compatible_acl = ONTOLOGY_ACL.replace(
        "Deterministic W0.1 Ontology contract fixture",
        "PostgreSQL-compatible description revision",
    );
    let compatible_revision_request = || {
        BootRequest::new(HttpMethod::Post, format!("{ontology_root}/revisions"))
            .with_header("content-type", "application/vnd.a3s.acl")
            .with_header("idempotency-key", "ontology-support-compatible")
            .with_header("x-a3s-expected-version", "1")
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
            .with_body(compatible_acl.as_bytes().to_vec())
    };
    let compatible_revision = app.call(compatible_revision_request()).await?;
    assert_eq!(compatible_revision.status(), 201);

    let breaking_acl = integration_breaking_ontology_acl(&compatible_acl);
    let breaking_without_migration = app
        .call(
            BootRequest::new(HttpMethod::Post, format!("{ontology_root}/revisions"))
                .with_header("content-type", "application/vnd.a3s.acl")
                .with_header("idempotency-key", "ontology-support-breaking-rejected")
                .with_header("x-a3s-expected-version", "2")
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
                .with_body(breaking_acl.as_bytes().to_vec()),
        )
        .await?;
    assert_eq!(breaking_without_migration.status(), 422);
    let explicit_migration = app
        .call(
            BootRequest::new(HttpMethod::Post, format!("{ontology_root}/revisions"))
                .with_header("content-type", "application/vnd.a3s.acl")
                .with_header("idempotency-key", "ontology-support-breaking")
                .with_header("x-a3s-expected-version", "2")
                .with_header("x-a3s-migration-rule", "migrate_ticket_v2")
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
                .with_body(breaking_acl.as_bytes().to_vec()),
        )
        .await?;
    assert_eq!(explicit_migration.status(), 201);
    assert_eq!(
        response_json(&explicit_migration)?["data"]["revision"]["migrationPolicy"]["kind"],
        "explicit"
    );
    let historical_create_replay = app.call(create_ontology()).await?;
    assert_eq!(historical_create_replay.status(), 200);
    assert_eq!(
        response_json(&historical_create_replay)?["data"]["ontology"]["currentRevisionNumber"],
        1
    );
    let historical_revision_replay = app.call(compatible_revision_request()).await?;
    assert_eq!(historical_revision_replay.status(), 200);
    let historical_revision_replay = response_json(&historical_revision_replay)?;
    assert_eq!(
        historical_revision_replay["data"]["ontology"]["currentRevisionNumber"],
        2
    );
    assert_eq!(
        historical_revision_replay["data"]["revision"]["revisionNumber"],
        2
    );
    let ontology_storage = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>(
                "select (select count(*) from ontologies where organization_id = ",
            )
            .bind(Uuid::parse_str(&organization_id)?)
            .append(" and id = ")
            .bind(Uuid::parse_str(&ontology_id)?)
            .append(" and aggregate_version = 3), (select count(*) from ontology_revisions where organization_id = ")
            .bind(Uuid::parse_str(&organization_id)?)
            .append(" and ontology_id = ")
            .bind(Uuid::parse_str(&ontology_id)?)
            .append("), (select count(*) from audit_records where organization_id = ")
            .bind(Uuid::parse_str(&organization_id)?)
            .append(" and aggregate_id = ")
            .bind(Uuid::parse_str(&ontology_id)?)
            .append(" and action in ('workflow.ontology.created', 'workflow.ontology.revised')), (select count(*) from authorized_search_projections where organization_id = ")
            .bind(Uuid::parse_str(&organization_id)?)
            .append(" and resource_kind = 'ontology' and resource_id = ")
            .bind(Uuid::parse_str(&ontology_id)?)
            .append(")"),
        )
        .await?;
    assert_eq!(ontology_storage, (1, 3, 3, 1));
    let immutable_revision = database
        .execute(
            sql_query::<()>("update ontology_revisions set canonical_acl = canonical_acl where organization_id = ")
                .bind(Uuid::parse_str(&organization_id)?)
                .append(" and ontology_id = ")
                .bind(Uuid::parse_str(&ontology_id)?),
        )
        .await;
    let immutable_revision = immutable_revision.expect_err("Ontology revision update must fail");
    assert!(matches!(
        immutable_revision,
        a3s_orm::DatabaseError::Execute(PostgresError::Database(ref error))
            if error
                .as_db_error()
                .is_some_and(|error| error.message() == "Ontology revisions are immutable")
    ));

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
    assets_support::exercise_assets(
        &executor,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        OrganizationId::from_uuid(Uuid::parse_str(&response_id(
            &installation_conflict_organization,
        )?)?),
    )
    .await?;
    let hosted_git_asset = assets_support::exercise_asset_git_controls(
        &executor,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        OrganizationId::from_uuid(Uuid::parse_str(&response_id(
            &installation_conflict_organization,
        )?)?),
    )
    .await?;
    let hosted_git =
        LocalAssetGitRepository::new(&asset_repository_directory, Duration::from_secs(10))?;
    hosted_git.provision(&hosted_git_asset).await?;
    let receive_advertisement_path = format!(
        "/api/v1/organizations/{organization_id}/assets/{}/git/info/refs?service=git-receive-pack",
        hosted_git_asset.id
    );
    let receive_advertisement = app
        .call(get_as(&receive_advertisement_path, ADMIN_TOKEN))
        .await?;
    assert_eq!(receive_advertisement.status(), 200);
    assert_eq!(
        receive_advertisement.header("content-type"),
        Some("application/x-git-receive-pack-advertisement")
    );
    assert!(receive_advertisement
        .body()
        .starts_with(b"001f# service=git-receive-pack\n0000"));
    let physical_repository = asset_repository_directory
        .join(organization_id.as_str())
        .join(format!("{}.git", hosted_git_asset.id));
    let original_refs = hosted_git.refs_digest(&hosted_git_asset).await?;
    let original_bytes = hosted_git.repository_bytes(&hosted_git_asset).await?;
    let crash_body =
        assets_support::receive_pack_fixture(&physical_repository, hosted_git_asset.kind)?;
    let crash_acquired_at = Utc::now();
    let crash_controls = PostgresAssetRepository::new(executor.clone());
    let crash_lease = crash_controls
        .acquire_write(AcquireAssetGitWriteLease {
            asset: hosted_git_asset.clone(),
            lease_id: Uuid::now_v7(),
            operation: AssetGitWriteOperation::ReceivePack,
            actor_id: Uuid::now_v7(),
            request_id: Uuid::now_v7(),
            observed_bytes: original_bytes,
            default_quota_bytes: 1_048_576,
            acquired_at: crash_acquired_at,
            leased_until: crash_acquired_at + chrono::Duration::seconds(1),
        })
        .await?;
    hosted_git
        .prepare_write(&hosted_git_asset, &crash_lease)
        .await?;
    hosted_git
        .execute_rpc(
            &hosted_git_asset,
            AssetGitService::ReceivePack,
            crash_body,
            AssetGitRpcLimits {
                maximum_input_bytes: 64 * 1024 * 1024,
                maximum_repository_bytes: crash_lease.quota_bytes,
            },
            Some(&crash_lease),
        )
        .await?;
    assert_ne!(
        hosted_git.refs_digest(&hosted_git_asset).await?,
        original_refs
    );
    drop(hosted_git);
    let restarted_git =
        LocalAssetGitRepository::new(&asset_repository_directory, Duration::from_secs(10))?;
    let recovery = match crash_controls
        .claim_write_recovery(ClaimAssetGitWriteRecovery {
            asset: hosted_git_asset.clone(),
            claimed_at: crash_acquired_at + chrono::Duration::seconds(2),
            leased_until: crash_acquired_at + chrono::Duration::seconds(32),
        })
        .await?
    {
        Some(AssetGitWriteRecovery::Rollback(lease)) => lease,
        outcome => return Err(format!("unexpected real hosted Git recovery: {outcome:?}").into()),
    };
    assert_eq!(recovery.lease_id, crash_lease.lease_id);
    restarted_git
        .rollback_write(&hosted_git_asset, &recovery)
        .await?;
    crash_controls.abandon_write(&recovery).await?;
    assert_eq!(
        restarted_git.refs_digest(&hosted_git_asset).await?,
        original_refs
    );
    assert!(restarted_git.repository_bytes(&hosted_git_asset).await? <= original_bytes);
    let receive_body =
        assets_support::receive_pack_fixture(&physical_repository, hosted_git_asset.kind)?;
    let receive = app
        .call(
            BootRequest::new(
                HttpMethod::Post,
                format!(
                    "/api/v1/organizations/{organization_id}/assets/{}/git/git-receive-pack",
                    hosted_git_asset.id
                ),
            )
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
            .with_header("content-type", "application/x-git-receive-pack-request")
            .with_body(receive_body),
        )
        .await?;
    assert_eq!(receive.status(), 200);
    assert_eq!(
        receive.header("content-type"),
        Some("application/x-git-receive-pack-result")
    );
    assert!(!receive.body().is_empty());
    assert_eq!(
        database
            .fetch_one_as(sql_query::<(Option<Uuid>, Option<Uuid>)>(
                "select write_lease_id, write_cleanup_lease_id from asset_git_repository_controls where organization_id = ",
            )
            .bind(hosted_git_asset.organization_id.as_uuid())
            .append(" and asset_id = ")
            .bind(hosted_git_asset.id.as_uuid()))
            .await?,
        (None, None),
        "successful Smart HTTP completion must settle the same write journal",
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from audit_records where aggregate_id = ",)
                    .bind(hosted_git_asset.id.as_uuid())
                    .append(" and action = ")
                    .bind("asset.repository.pushed")
            )
            .await?,
        2,
        "repository-control completion and Smart HTTP push must each use the shared audit table",
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ",)
                    .bind(hosted_git_asset.id.as_uuid())
                    .append(" and event_key = ")
                    .bind("asset.asset.created")
            )
            .await?,
        1,
        "Hosted Git controls must reuse the Asset outbox event instead of publishing a second repository event",
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from idempotency_records where scope_key = ",)
                    .bind(format!("organizations/{organization_id}/assets"))
                    .append(" and idempotency_key = ")
                    .bind("create-hosted-git-control")
            )
            .await?,
        1,
        "Hosted Git controls must reuse the Asset idempotency authority",
    );

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
    assert_eq!(
        source.status(),
        201,
        "unexpected source revision response: {}",
        response_json(&source)?
    );
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
    assert_eq!(
        moved_delivery.status(),
        409,
        "unexpected moved source revision response: {}",
        response_json(&moved_delivery)?,
    );
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

    build_runs_support::exercise_build_run_persistence(
        &app,
        &executor,
        &organization_id,
        &project_id,
        &environment_id,
        &response_id(&source)?,
    )
    .await?;
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
    assert_eq!(hashed_token_rows, 3);

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
    assert_eq!((outbox_events, idempotency_records), (55, 41));

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
    assert_eq!(lost_ack.claimed, 2);
    assert_eq!(lost_ack.published, 1);
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
    assert_eq!(local_events.len(), initial_event_count + 3);
    let unique_event_ids = local_events
        .iter()
        .map(|event| event.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_event_ids.len(), initial_event_count + 2);

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
    .await
    .map_err(|error| format!("Secret rotation restart integration failed: {error}"))?;
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

    let listed_workloads = app.call(get_as(&workload_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed_workloads.status(), 200);
    let listed = &response_json(&listed_workloads)?["data"];
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    assert_eq!(listed[0]["id"], workload_id);
    assert_eq!(listed[0]["desiredRevision"]["generation"], 2);
    assert_eq!(listed[0]["activeRevision"]["generation"], 2);
    assert_eq!(listed[0]["control"]["managedOwner"], Value::Null);
    assert_eq!(
        listed[0]["control"]["placementPolicy"]["schema"],
        "a3s.cloud.effective-placement-policy.v3"
    );
    assert_eq!(
        listed[0]["control"]["placementPolicy"]["desiredReplicas"],
        1
    );
    assert_eq!(
        listed[0]["control"]["placementPolicy"]["replicaAntiAffinity"],
        "required"
    );
    assert_eq!(
        listed[0]["control"]["placementPolicy"]["nodePoolId"],
        Value::Null
    );
    assert_eq!(listed[0]["replicas"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed[0]["replicas"][0]["id"], workload_id);
    assert_eq!(listed[0]["replicas"][0]["revisionGeneration"], 2);
    assert_eq!(listed[0]["replicas"][0]["generation"], 2);
    assert_eq!(listed[0]["replicas"][0]["lifecycle"], "desired");
    assert_eq!(listed[0]["replicas"][0]["evacuationNodeId"], Value::Null);
    assert_eq!(listed[0]["replicas"][0]["retirementCommandId"], Value::Null);
    assert_eq!(listed[0]["replicas"][0]["runtimeFencedAt"], Value::Null);
    assert_eq!(
        listed[0]["replicas"][0]["members"][0]["nodeId"],
        listed[0]["deployments"][0]["nodeId"]
    );
    assert!(
        listed[0]["replicas"][0]["members"][0]["placementGeneration"]
            .as_u64()
            .is_some_and(|generation| generation > 0)
    );
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
            workload_id: &workload_id,
            workload_revision_id: &restart_revision_id,
            runtime_generation: restart_fixture.generation,
            node_id: deployment_flow_fixture.node_id,
            token: ADMIN_TOKEN,
        },
    )
    .await
    .map_err(|error| format!("Edge API integration failed after Secret restart: {error}"))?;

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
    .await
    .map_err(|error| format!("deployment cancellation integration failed: {error}"))?;

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
    .await
    .map_err(|error| format!("workload rollback acceptance failed: {error}"))?;

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
    .await
    .map_err(|error| format!("workload rollback replay failed after stop: {error}"))?;

    fleet_support::exercise_fleet(&executor, Uuid::parse_str(&organization_id)?)
        .await
        .map_err(|error| format!("Fleet persistence integration failed: {error}"))?;
    let workload_fixture = workloads_support::exercise_workloads(
        &executor,
        Uuid::parse_str(&organization_id)?,
        Uuid::parse_str(&project_id)?,
        Uuid::parse_str(&environment_id)?,
    )
    .await
    .map_err(|error| format!("Workload persistence integration failed: {error}"))?;
    let mut replica_set_fixture = workloads_support::exercise_replica_set(
        &executor,
        Uuid::parse_str(&organization_id)?,
        Uuid::parse_str(&project_id)?,
        Uuid::parse_str(&environment_id)?,
    )
    .await
    .map_err(|error| format!("Workload replica-set persistence failed: {error}"))?;
    workloads_support::exercise_replica_evacuation(
        &executor,
        Uuid::parse_str(&organization_id)?,
        &mut replica_set_fixture,
    )
    .await
    .map_err(|error| format!("Workload replica evacuation failed: {error}"))?;
    resource_claims_support::exercise_resource_claims(
        &executor,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        &workload_fixture,
        &replica_set_fixture,
    )
    .await
    .map_err(|error| format!("Resource Claim persistence integration failed: {error}"))?;
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
            revision_generation: workload_fixture.revision_generation,
            candidate_revision_id: workload_fixture.candidate_revision_id,
            candidate_generation: workload_fixture.candidate_generation,
            candidate_deployment_id: workload_fixture.candidate_deployment_id,
        },
    )
    .await
    .map_err(|error| format!("Edge persistence integration failed: {error}"))?;
    let gateway_rollout_fixture = gateway_rollouts_support::GatewayRolloutFixture {
        organization_id: OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        project_id: a3s_cloud_control_plane::modules::shared_kernel::domain::ProjectId::from_uuid(
            Uuid::parse_str(&project_id)?,
        ),
        environment_id:
            a3s_cloud_control_plane::modules::shared_kernel::domain::EnvironmentId::from_uuid(
                Uuid::parse_str(&environment_id)?,
            ),
        workload_id: workload_fixture.workload_id,
        workload_revision_id: workload_fixture.revision_id,
        workload_revision_generation: workload_fixture.revision_generation,
    };
    gateway_replica_recovery_support::exercise_gateway_replica_recovery(
        &executor,
        gateway_rollout_fixture,
    )
    .await
    .map_err(|error| format!("Gateway replica recovery integration failed: {error}"))?;
    gateway_rollouts_support::exercise_replicated_gateway_rollout(
        &executor,
        gateway_rollout_fixture,
    )
    .await
    .map_err(|error| format!("Gateway rollout persistence integration failed: {error}"))?;
    mcp_route_policies_support::exercise(
        &executor,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        OrganizationId::from_uuid(Uuid::parse_str(&response_id(
            &installation_conflict_organization,
        )?)?),
        ProjectId::from_uuid(Uuid::parse_str(&project_id)?),
        a3s_cloud_control_plane::modules::shared_kernel::domain::EnvironmentId::from_uuid(
            Uuid::parse_str(&environment_id)?,
        ),
    )
    .await
    .map_err(|error| format!("MCP route-policy persistence integration failed: {error}"))?;
    executions_support::exercise_execution_persistence(
        &executor,
        OrganizationId::from_uuid(Uuid::parse_str(&organization_id)?),
        OrganizationId::from_uuid(Uuid::parse_str(&response_id(
            &installation_conflict_organization,
        )?)?),
        ProjectId::from_uuid(Uuid::parse_str(&project_id)?),
        a3s_cloud_control_plane::modules::shared_kernel::domain::EnvironmentId::from_uuid(
            Uuid::parse_str(&environment_id)?,
        ),
    )
    .await
    .map_err(|error| format!("Execution persistence integration failed: {error}"))?;

    Ok(())
}

async fn assert_route_target_migration_backfills_legacy_projection(
    executor: &PostgresExecutor,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = executor.pool().get().await?;
    let migration = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/035_route_target_generation.sql"
    ));
    let probe = format!(
        r#"
begin;

create temporary table workload_revisions (
    id uuid not null,
    workload_id uuid not null,
    generation bigint not null
);

create temporary table routes (
    workload_id uuid not null,
    workload_revision_id uuid not null,
    updated_at timestamptz not null
);

create temporary table gateway_route_cutovers (
    workload_id uuid not null,
    previous_revision_id uuid not null,
    candidate_revision_id uuid not null,
    routes jsonb not null
);

insert into workload_revisions (id, workload_id, generation) values
    (
        '10000000-0000-0000-0000-000000000011',
        '10000000-0000-0000-0000-000000000001',
        1
    ),
    (
        '10000000-0000-0000-0000-000000000012',
        '10000000-0000-0000-0000-000000000001',
        2
    );

insert into routes (workload_id, workload_revision_id, updated_at) values (
    '10000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000011',
    '2026-01-01T00:00:00Z'
);

insert into gateway_route_cutovers (
    workload_id,
    previous_revision_id,
    candidate_revision_id,
    routes
) values (
    '10000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000011',
    '10000000-0000-0000-0000-000000000012',
    '[{{
        "workload_id": "10000000-0000-0000-0000-000000000001",
        "workload_revision_id": "10000000-0000-0000-0000-000000000012",
        "updated_at": "2026-01-01T00:00:01Z"
    }}]'::jsonb
);

{migration}

do $probe$
begin
    if (
        select runtime_generation <> 1
            or runtime_unit_id <>
                'workload:10000000-0000-0000-0000-000000000001'
                    || ':revision:10000000-0000-0000-0000-000000000011'
            or target_observed_at <> '2026-01-01T00:00:00Z'::timestamptz
        from routes
    ) then
        raise exception 'route target migration did not backfill the authoritative route';
    end if;

    if (
        select previous_generation <> 1
            or candidate_generation <> 2
            or routes -> 0 ->> 'runtime_unit_id' <>
                'workload:10000000-0000-0000-0000-000000000001'
                    || ':revision:10000000-0000-0000-0000-000000000012'
            or (routes -> 0 ->> 'runtime_generation')::bigint <> 2
            or routes -> 0 ->> 'observed_at' <> '2026-01-01T00:00:01Z'
        from gateway_route_cutovers
    ) then
        raise exception 'route target migration did not backfill the cutover projection';
    end if;

    begin
        update routes set runtime_generation = 2;
        raise exception 'route target revision-generation constraint accepted a mismatch';
    exception
        when foreign_key_violation then null;
    end;

    begin
        update routes set runtime_unit_id = 'workload:forged:revision:forged';
        raise exception 'route target deterministic identity constraint accepted a mismatch';
    exception
        when check_violation then null;
    end;

    begin
        update gateway_route_cutovers set candidate_generation = previous_generation;
        raise exception 'route cutover generation ordering constraint accepted a mismatch';
    exception
        when check_violation then null;
    end;
end
$probe$;

rollback;
"#
    );
    client.batch_execute(&probe).await?;
    Ok(())
}

async fn assert_logical_gateway_scope_migration_backfills_legacy_projection(
    executor: &PostgresExecutor,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = executor.pool().get().await?;
    let migration = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/036_logical_gateway_scopes.sql"
    ));
    let probe = format!(
        r#"
begin;
set local search_path = pg_temp;

create temporary table environments (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    unique (organization_id, project_id, id)
);

create temporary table nodes (
    organization_id uuid not null,
    id uuid not null,
    unique (organization_id, id)
);

create temporary table routes (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    gateway_node_id uuid not null,
    hostname text not null,
    path_prefix text not null,
    state text not null,
    created_at timestamptz not null,
    updated_at timestamptz not null
);

create temporary table gateway_route_cutovers (
    routes jsonb not null
);

create temporary table gateway_certificate_convergences (
    retained_routes jsonb not null,
    rejected_routes jsonb not null
);

create temporary table idempotency_records (
    response jsonb not null
);

insert into environments (organization_id, project_id, id) values
    (
        '20000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000002',
        '20000000-0000-0000-0000-000000000003'
    ),
    (
        '20000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000002',
        '20000000-0000-0000-0000-000000000004'
    );

insert into nodes (organization_id, id) values
    (
        '20000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000005'
    ),
    (
        '20000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000006'
    );

insert into routes (
    id,
    organization_id,
    project_id,
    environment_id,
    gateway_node_id,
    hostname,
    path_prefix,
    state,
    created_at,
    updated_at
) values
    (
        '20000000-0000-0000-0000-000000000011',
        '20000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000002',
        '20000000-0000-0000-0000-000000000003',
        '20000000-0000-0000-0000-000000000005',
        'api.example.com',
        '/',
        'active',
        '2026-01-01T00:00:00Z',
        '2026-01-01T00:00:01Z'
    ),
    (
        '20000000-0000-0000-0000-000000000012',
        '20000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000002',
        '20000000-0000-0000-0000-000000000004',
        '20000000-0000-0000-0000-000000000005',
        'web.example.com',
        '/',
        'active',
        '2026-01-01T00:00:02Z',
        '2026-01-01T00:00:03Z'
    );

insert into gateway_route_cutovers (routes) values (
    '[{{"id":"20000000-0000-0000-0000-000000000011"}}]'::jsonb
);

insert into gateway_certificate_convergences (
    retained_routes,
    rejected_routes
) values (
    '[
        {{
            "route_id": "20000000-0000-0000-0000-000000000011",
            "aggregate_version": 3
        }}
    ]'::jsonb,
    '[
        {{
            "route_id": "20000000-0000-0000-0000-000000000012",
            "aggregate_version": 4
        }}
    ]'::jsonb
);

insert into idempotency_records (response) values
    (
        '{{"route":{{"id":"20000000-0000-0000-0000-000000000011"}}}}'::jsonb
    ),
    (
        '{{"cutover":{{"routes":[{{"id":"20000000-0000-0000-0000-000000000012"}}]}}}}'::jsonb
    );

{migration}

do $probe$
declare
    first_scope uuid;
    second_scope uuid;
begin
    if (select count(*) from gateway_route_scopes) <> 2
        or (select count(distinct id) from gateway_route_scopes) <> 2
    then
        raise exception 'logical Gateway scope migration did not split legacy environments';
    end if;

    select gateway_scope_id into first_scope
    from routes
    where id = '20000000-0000-0000-0000-000000000011';
    select gateway_scope_id into second_scope
    from routes
    where id = '20000000-0000-0000-0000-000000000012';

    if first_scope is null or second_scope is null or first_scope = second_scope then
        raise exception 'logical Gateway scope migration did not bind legacy routes';
    end if;

    if not exists (
        select 1
        from gateway_route_cutovers
        where routes -> 0 ->> 'gateway_scope_id' = first_scope::text
    ) or not exists (
        select 1
        from gateway_certificate_convergences
        where retained_routes = '[
                {{
                    "route_id": "20000000-0000-0000-0000-000000000011",
                    "aggregate_version": 3
                }}
            ]'::jsonb
            and rejected_routes = '[
                {{
                    "route_id": "20000000-0000-0000-0000-000000000012",
                    "aggregate_version": 4
                }}
            ]'::jsonb
    ) or not exists (
        select 1
        from idempotency_records
        where response #>> '{{route,gateway_scope_id}}' = first_scope::text
    ) or not exists (
        select 1
        from idempotency_records
        where response #>> '{{cutover,routes,0,gateway_scope_id}}' = second_scope::text
    ) then
        raise exception 'logical Gateway scope migration left a serialized route unbound';
    end if;

    begin
        update routes
        set gateway_scope_id = second_scope
        where id = '20000000-0000-0000-0000-000000000011';
        raise exception 'logical Gateway scope tenancy constraint accepted a cross-environment route';
    exception
        when foreign_key_violation then null;
    end;

    begin
        update routes
        set gateway_node_id = '20000000-0000-0000-0000-000000000006'
        where id = '20000000-0000-0000-0000-000000000011';
        raise exception 'logical Gateway scope binding accepted the wrong node';
    exception
        when foreign_key_violation then null;
    end;
end
$probe$;

rollback;
"#
    );
    client.batch_execute(&probe).await?;
    Ok(())
}

async fn assert_gateway_management_protocol_migration_preserves_legacy_acknowledgements(
    executor: &PostgresExecutor,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = executor.pool().get().await?;
    let migration = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/037_gateway_management_protocol.sql"
    ));
    let probe = format!(
        r#"
begin;
set local search_path = pg_temp;

create temporary table node_gateway_acknowledgements (
    acknowledgement_id uuid primary key
);

insert into node_gateway_acknowledgements (acknowledgement_id)
values ('37000000-0000-0000-0000-000000000001');

{migration}

do $$
begin
    if exists (
        select 1
        from node_gateway_acknowledgements
        where acknowledgement_id = '37000000-0000-0000-0000-000000000001'
          and (
              management_protocol is not null
              or snapshot_request_schema is not null
              or snapshot_status_schema is not null
              or protocol_discovery is not null
          )
    ) then
        raise exception 'legacy Gateway acknowledgement gained invented protocol evidence';
    end if;

    update node_gateway_acknowledgements
    set management_protocol = 'a3s.gateway.management-protocol.v1',
        snapshot_request_schema = 'a3s.gateway.managed-snapshot.v1',
        snapshot_status_schema = 'a3s.gateway.managed-snapshot-status.v1',
        protocol_discovery = 'advertised'
    where acknowledgement_id = '37000000-0000-0000-0000-000000000001';

    begin
        update node_gateway_acknowledgements
        set snapshot_status_schema = 'a3s.gateway.managed-snapshot-status.v2'
        where acknowledgement_id = '37000000-0000-0000-0000-000000000001';
        raise exception 'incompatible Gateway protocol evidence was accepted';
    exception
        when check_violation then null;
    end;
end
$$;

rollback;
"#
    );
    client.batch_execute(&probe).await?;
    Ok(())
}

async fn assert_gateway_scope_membership_migration_backfills_primary_members(
    executor: &PostgresExecutor,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = executor.pool().get().await?;
    let migration = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/038_gateway_scope_membership.sql"
    ));
    let probe = format!(
        r#"
begin;
set local search_path = pg_temp;

create temporary table nodes (
    id uuid primary key,
    organization_id uuid not null,
    unique (organization_id, id)
);

create temporary table gateway_route_scopes (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    node_id uuid not null,
    aggregate_version bigint not null,
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, project_id, environment_id, node_id),
    unique (id, organization_id, project_id, environment_id, node_id)
);

create temporary table idempotency_records (
    scope_key text not null,
    idempotency_key text not null,
    request_digest text not null,
    response jsonb not null,
    created_at timestamptz not null,
    primary key (scope_key, idempotency_key)
);

insert into nodes (id, organization_id)
values (
    '38000000-0000-0000-0000-000000000002',
    '38000000-0000-0000-0000-000000000003'
);

insert into gateway_route_scopes (
    id,
    organization_id,
    project_id,
    environment_id,
    node_id,
    aggregate_version,
    created_at,
    updated_at
)
values (
    '38000000-0000-0000-0000-000000000001',
    '38000000-0000-0000-0000-000000000003',
    '38000000-0000-0000-0000-000000000004',
    '38000000-0000-0000-0000-000000000005',
    '38000000-0000-0000-0000-000000000002',
    1,
    '2026-07-25T00:00:00Z',
    '2026-07-25T00:00:00Z'
);

insert into idempotency_records (
    scope_key,
    idempotency_key,
    request_digest,
    response,
    created_at
)
values (
    'organizations/38000000-0000-0000-0000-000000000003/projects/38000000-0000-0000-0000-000000000004/environments/38000000-0000-0000-0000-000000000005/gateway-scopes',
    'legacy-scope',
    'sha256:legacy',
    jsonb_build_object(
        'id',
        '38000000-0000-0000-0000-000000000001',
        'organization_id',
        '38000000-0000-0000-0000-000000000003',
        'project_id',
        '38000000-0000-0000-0000-000000000004',
        'environment_id',
        '38000000-0000-0000-0000-000000000005',
        'node_id',
        '38000000-0000-0000-0000-000000000002',
        'aggregate_version',
        1,
        'created_at',
        '2026-07-25T00:00:00Z',
        'updated_at',
        '2026-07-25T00:00:00Z'
    ),
    '2026-07-25T00:00:00Z'
);

{migration}

do $$
begin
    if not exists (
        select 1
        from gateway_scope_members
        where gateway_scope_id = '38000000-0000-0000-0000-000000000001'
          and node_id = '38000000-0000-0000-0000-000000000002'
          and ordinal = 0
          and membership_generation = 1
    ) then
        raise exception 'Gateway scope migration did not backfill its primary member';
    end if;

    if not exists (
        select 1
        from gateway_route_scopes
        where id = '38000000-0000-0000-0000-000000000001'
          and membership_generation = 1
          and min_ready = 1
          and max_unavailable = 0
    ) then
        raise exception 'Gateway scope migration did not backfill rollout policy';
    end if;

    if not exists (
        select 1
        from idempotency_records
        where idempotency_key = 'legacy-scope'
          and response -> 'member_node_ids'
              = jsonb_build_array('38000000-0000-0000-0000-000000000002')
          and response ->> 'membership_generation' = '1'
          and response #>> '{{rollout_policy,min_ready}}' = '1'
          and response #>> '{{rollout_policy,max_unavailable}}' = '0'
    ) then
        raise exception 'Gateway scope migration did not upgrade replay documents';
    end if;
end
$$;

rollback;
"#
    );
    client.batch_execute(&probe).await?;
    Ok(())
}

async fn assert_box_native_build_authority_migration_invalidates_legacy_runs(
    executor: &PostgresExecutor,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = executor.pool().get().await?;
    let migration = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/060_box_native_build_authority.sql"
    ));
    let probe = format!(
        r#"
begin;
set local search_path = pg_temp;

create temporary table build_runs (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    source_revision_id uuid not null,
    operation_id uuid not null,
    status text not null,
    source_content_digest text,
    input_artifact jsonb,
    node_id uuid,
    command_id uuid,
    cleanup_command_id uuid,
    runtime_spec_digest text,
    runtime_output_artifact jsonb,
    output jsonb,
    publication_target jsonb,
    published_artifact jsonb,
    evidence_required boolean not null,
    evidence jsonb,
    cache_required boolean not null,
    cache jsonb,
    failure text,
    aggregate_version bigint not null,
    attempt integer not null,
    retry_of_build_run_id uuid,
    requested_at timestamptz not null,
    updated_at timestamptz not null,
    started_at timestamptz,
    cancellation_requested_at timestamptz,
    finished_at timestamptz,
    check (status <> '')
);

create temporary table operation_projections (
    operation_id uuid primary key,
    status text not null,
    last_sequence bigint not null,
    output jsonb,
    error text,
    updated_at timestamptz not null
);

create temporary table operation_requests (
    operation_id uuid primary key
);

insert into build_runs (
    organization_id,
    project_id,
    environment_id,
    id,
    source_revision_id,
    operation_id,
    status,
    source_content_digest,
    input_artifact,
    node_id,
    command_id,
    cleanup_command_id,
    runtime_spec_digest,
    runtime_output_artifact,
    output,
    publication_target,
    published_artifact,
    evidence_required,
    evidence,
    cache_required,
    cache,
    aggregate_version,
    attempt,
    requested_at,
    updated_at,
    started_at,
    finished_at
) values (
    '60000000-0000-0000-0000-000000000001',
    '60000000-0000-0000-0000-000000000002',
    '60000000-0000-0000-0000-000000000003',
    '60000000-0000-0000-0000-000000000004',
    '60000000-0000-0000-0000-000000000005',
    '60000000-0000-0000-0000-000000000004',
    'succeeded',
    'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    '{{
        "uri": "a3s-cloud-artifact://sha256/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "mediaType": "application/vnd.a3s.directory.v1+tar",
        "sizeBytes": 1024
    }}'::jsonb,
    '60000000-0000-0000-0000-000000000006',
    '60000000-0000-0000-0000-000000000007',
    '60000000-0000-0000-0000-000000000008',
    'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    '{{"legacy": "runtime-output"}}'::jsonb,
    '{{"legacy": "validated-output"}}'::jsonb,
    '{{"legacy": "publication-target"}}'::jsonb,
    '{{"legacy": "published-artifact"}}'::jsonb,
    true,
    '{{"legacy": "evidence"}}'::jsonb,
    true,
    '{{"legacy": "cache"}}'::jsonb,
    7,
    1,
    '2026-07-01T00:00:00Z',
    '2026-07-01T00:00:08Z',
    '2026-07-01T00:00:01Z',
    '2026-07-01T00:00:08Z'
), (
    '60000000-0000-0000-0000-000000000001',
    '60000000-0000-0000-0000-000000000002',
    '60000000-0000-0000-0000-000000000003',
    '60000000-0000-0000-0000-000000000009',
    '60000000-0000-0000-0000-000000000010',
    '60000000-0000-0000-0000-000000000009',
    'queued',
    null,
    null,
    null,
    null,
    null,
    null,
    null,
    null,
    null,
    null,
    false,
    null,
    false,
    null,
    1,
    1,
    '2026-07-01T00:00:00Z',
    '2026-07-01T00:00:00Z',
    null,
    null
);

insert into operation_requests (operation_id)
select operation_id from build_runs;

insert into operation_projections (
    operation_id,
    status,
    last_sequence,
    output,
    error,
    updated_at
)
select operation_id, status, 7, '{{"legacy": true}}'::jsonb, null, updated_at
from build_runs
where id = '60000000-0000-0000-0000-000000000004';

{migration}

do $probe$
declare
    invalidation_reason constant text :=
        'build predates the sole Box-native workflow; rebuild required';
begin
    if (select count(*) from build_runs) <> 2
        or exists (
            select 1
            from build_runs
            where status <> 'failed'
               or node_id is not null
               or command_id is not null
               or cleanup_command_id is not null
               or build_request_digest is not null
               or box_build_output is not null
               or output is not null
               or publication_target is not null
               or published_artifact is not null
               or evidence is not null
               or failure <> invalidation_reason
               or cancellation_requested_at is not null
               or finished_at is null
        )
    then
        raise exception 'Box authority migration retained a legacy build projection';
    end if;

    if not exists (
        select 1
        from build_runs
        where id = '60000000-0000-0000-0000-000000000004'
          and aggregate_version = 8
          and source_content_digest =
              'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
          and input_artifact ->> 'digest' = source_content_digest
    ) or not exists (
        select 1
        from build_runs
        where id = '60000000-0000-0000-0000-000000000009'
          and aggregate_version = 2
    ) then
        raise exception 'Box authority migration changed immutable legacy input or version';
    end if;

    if (select count(*) from operation_projections) <> 2
        or exists (
        select 1
        from operation_projections as projection
        join build_runs as build
          on build.operation_id = projection.operation_id
        where projection.status <> 'cancelled'
           or projection.output is not null
           or projection.error <> invalidation_reason
           or projection.updated_at <> build.updated_at
    ) then
        raise exception 'Box authority migration retained a runnable operation projection';
    end if;

    if not exists (
        select 1
        from operation_projections
        where operation_id = '60000000-0000-0000-0000-000000000004'
          and last_sequence = 7
    ) or not exists (
        select 1
        from operation_projections
        where operation_id = '60000000-0000-0000-0000-000000000009'
          and last_sequence = 0
    ) then
        raise exception 'Box authority migration did not preserve or seed Flow projection sequences';
    end if;

    if exists (
        select 1
        from pg_attribute
        where attrelid = 'build_runs'::regclass
          and attnum > 0
          and not attisdropped
          and attname in (
              'runtime_spec_digest',
              'runtime_output_artifact',
              'cache_required',
              'cache'
          )
    ) then
        raise exception 'Box authority migration retained a duplicate legacy column';
    end if;

    update build_runs
    set status = 'running',
        source_content_digest =
            'sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
        input_artifact = '{{
            "uri": "a3s-cloud-artifact://sha256/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "digest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "mediaType": "application/vnd.a3s.directory.v1+tar",
            "sizeBytes": 1024
        }}'::jsonb,
        node_id = '60000000-0000-0000-0000-000000000011',
        command_id = '60000000-0000-0000-0000-000000000012',
        build_request_digest =
            'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
        failure = null,
        started_at = requested_at,
        finished_at = null
    where id = '60000000-0000-0000-0000-000000000009';

    begin
        update build_runs
        set cleanup_command_id = '60000000-0000-0000-0000-000000000013'
        where id = '60000000-0000-0000-0000-000000000009';
        raise exception 'running Box build accepted a premature cleanup command';
    exception
        when check_violation then null;
    end;
end
$probe$;

rollback;
"#
    );
    client.batch_execute(&probe).await?;
    Ok(())
}
