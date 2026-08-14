use super::postgres_schema::{AuditRecords, IdempotencyRecords, MigrationRecords, OutboxEvents};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NodeId, OrganizationId, RepositoryError,
};
use a3s_boot::HealthIndicatorResult;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::migration::MigrationRunError;
use a3s_orm::{
    insert_into, select_from, Database, DecodeError, Executor, FromRow, Migration, Migrator,
    PostgresDialect, PostgresError, PostgresExecutor, PostgresMigrationError, PostgresTransaction,
    PostgresTransactionError, Query,
};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

pub(crate) struct AuditWrite {
    pub(crate) audit_id: Uuid,
    pub(crate) organization_id: Uuid,
    pub(crate) actor_id: Option<Uuid>,
    pub(crate) action: &'static str,
    pub(crate) aggregate_id: Uuid,
    pub(crate) occurred_at: DateTime<Utc>,
    pub(crate) request_id: Uuid,
    pub(crate) details: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum PostgresBootstrapError {
    #[error("could not configure PostgreSQL: {0}")]
    Connect(#[from] PostgresError),
    #[error("could not migrate PostgreSQL: {0}")]
    Migrate(#[from] MigrationRunError<PostgresMigrationError>),
    #[error("PostgreSQL did not become ready: {0}")]
    Readiness(String),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PostgresPersistenceError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("could not build PostgreSQL query: {0}")]
    Query(#[from] a3s_orm::Error),
    #[error("PostgreSQL query failed: {0}")]
    Database(#[from] PostgresError),
    #[error("could not decode PostgreSQL row: {0}")]
    Decode(#[from] DecodeError),
    #[error("could not serialize persisted response: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("PostgreSQL query returned {actual} rows where at most one was expected")]
    Cardinality { actual: usize },
    #[error("PostgreSQL persistence invariant failed: {0}")]
    Invariant(String),
}

impl PostgresPersistenceError {
    fn into_repository(self) -> RepositoryError {
        match self {
            Self::Repository(error) => error,
            error => RepositoryError::Storage(error.to_string()),
        }
    }
}

pub async fn connect_and_migrate(
    url: &str,
    max_connections: usize,
) -> Result<PostgresExecutor, PostgresBootstrapError> {
    let executor = PostgresExecutor::connect_no_tls(url, max_connections)?;
    Migrator::new(executor.clone())
        .run(cloud_migrations())
        .await?;
    verify_postgres(&executor).await?;
    Ok(executor)
}

fn cloud_migrations() -> Vec<Migration> {
    vec![
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
            "Agent Workload release bindings",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/066_agent_workload_release_bindings.sql"
            )),
        ),
        Migration::new(
            "067",
            "Skill Workload revision bindings",
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
            "hosted MCP credential delivery receipts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/071_mcp_credential_delivery_receipts.sql"
            )),
        ),
        Migration::new(
            "072",
            "hosted MCP Gateway node-wide logical scope evidence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/072_mcp_gateway_node_scope_evidence.sql"
            )),
        ),
        Migration::new(
            "073",
            "unified Gateway snapshot publication ownership",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/073_gateway_snapshot_publication_owners.sql"
            )),
        ),
        Migration::new(
            "074",
            "Identity principals and organization memberships",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/074_identity_principals_and_memberships.sql"
            )),
        ),
        Migration::new(
            "075",
            "versioned Workflow Ontologies",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/075_versioned_ontologies.sql"
            )),
        ),
        Migration::new(
            "076",
            "versioned Workflow definitions, goals, and plans",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/076_workflow_definitions_goals_and_plans.sql"
            )),
        ),
        Migration::new(
            "077",
            "cleanup-aware Flow operation cancellation",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/077_flow_operation_cancelling.sql"
            )),
        ),
        Migration::new(
            "078",
            "Boot Flow task queue schema",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/078_boot_flow_task_queue_schema.sql"
            )),
        ),
        Migration::new(
            "079",
            "Form drafts and immutable releases",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/079_form_drafts_and_releases.sql"
            )),
        ),
        Migration::new(
            "080",
            "Workflow runs and semantic step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/080_workflow_runs.sql"
            )),
        ),
        Migration::new(
            "081",
            "Human tasks, Form submissions, and durable Flow resume delivery",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/081_human_tasks_and_form_submissions.sql"
            )),
        ),
        Migration::new(
            "082",
            "Agent execution change-set projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/082_agent_execution_change_sets.sql"
            )),
        ),
        Migration::new(
            "083",
            "reviewed Plugin Host enablement plans",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/083_plugin_host_enablement_plans.sql"
            )),
        ),
        Migration::new(
            "084",
            "tenant plugin registry enrollment",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/084_plugin_registries.sql"
            )),
        ),
        Migration::new(
            "085",
            "Plugin Registry authorized Search projection",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/085_plugin_registry_search_projection.sql"
            )),
        ),
        Migration::new(
            "086",
            "stable multi-replica workload identity",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/086_workload_replica_sets.sql"
            )),
        ),
        Migration::new(
            "087",
            "Membership-bound Resource Grants",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/087_resource_grants.sql"
            )),
        ),
        Migration::new(
            "088",
            "required Workload replica anti-affinity",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/088_required_replica_anti_affinity.sql"
            )),
        ),
        Migration::new(
            "089",
            "durable Workload replica retirement evidence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/089_workload_replica_retirement_evidence.sql"
            )),
        ),
        Migration::new(
            "090",
            "durable Workload replica evacuation intent",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/090_workload_replica_evacuation_intent.sql"
            )),
        ),
        Migration::new(
            "091",
            "Fleet node pools and maintenance windows",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/091_fleet_node_pools.sql"
            )),
        ),
        Migration::new(
            "092",
            "Workload node pool selection",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/092_workload_node_pool_selection.sql"
            )),
        ),
        Migration::new(
            "093",
            "safe node pool member removal",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/093_safe_node_pool_member_removal.sql"
            )),
        ),
        Migration::new(
            "094",
            "Workload placement-group plans",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/094_workload_placement_group_plans.sql"
            )),
        ),
        Migration::new(
            "095",
            "Workload placement-group Deployments",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/095_workload_group_deployments.sql"
            )),
        ),
        Migration::new(
            "096",
            "HumanTask expiry coordination",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/096_human_task_expiry_coordination.sql"
            )),
        ),
        Migration::new(
            "097",
            "HumanTask parent cancellation coordination",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/097_human_task_parent_cancellation.sql"
            )),
        ),
        Migration::new(
            "098",
            "immutable ExecutionTemplate revisions",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/098_execution_template_revisions.sql"
            )),
        ),
        Migration::new(
            "099",
            "Workflow finite-task Execution bindings",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/099_workflow_execution_bindings.sql"
            )),
        ),
        Migration::new(
            "100",
            "Workflow finite-task step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/100_workflow_execution_step_projections.sql"
            )),
        ),
        Migration::new(
            "101",
            "membership invitations",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/101_membership_invitations.sql"
            )),
        ),
        Migration::new(
            "102",
            "external OIDC identity links and one-time flows",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/102_external_oidc_identity.sql"
            )),
        ),
        Migration::new(
            "103",
            "Workflow revision semantic contracts and plan v2",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/103_workflow_semantic_contracts.sql"
            )),
        ),
        Migration::new(
            "104",
            "immutable project attribution profiles",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/104_project_attribution_profiles.sql"
            )),
        ),
        Migration::new(
            "105",
            "WorkflowRun input v2 capacity",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/105_workflow_run_input_v2.sql"
            )),
        ),
        Migration::new(
            "106",
            "deduplicated notification inbox projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/106_notification_inbox.sql"
            )),
        ),
        Migration::new(
            "107",
            "immutable Workflow variable default material",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/107_workflow_variable_defaults.sql"
            )),
        ),
        Migration::new(
            "108",
            "immutable Workflow composite-region policies",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/108_workflow_composite_regions.sql"
            )),
        ),
    ]
}

async fn verify_postgres(executor: &PostgresExecutor) -> Result<(), PostgresBootstrapError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(readiness_query())
        .await
        .map(|_| ())
        .map_err(|error| PostgresBootstrapError::Readiness(error.to_string()))
}

pub async fn postgres_health(executor: PostgresExecutor) -> HealthIndicatorResult {
    match Database::new(PostgresDialect, executor)
        .fetch_one_as(readiness_query())
        .await
    {
        Ok(_) => HealthIndicatorResult::up(),
        Err(error) => HealthIndicatorResult::down().with_detail_value("error", error.to_string()),
    }
}

fn readiness_query() -> a3s_orm::query::SelectQuery<MigrationRecords, String> {
    select_from::<MigrationRecords>()
        .select(MigrationRecords::version())
        .limit(1)
}

pub(crate) async fn execute<Q>(
    transaction: &PostgresTransaction,
    query: Q,
) -> Result<u64, PostgresPersistenceError>
where
    Q: Query,
{
    let query = query.compile(&PostgresDialect)?;
    Ok(transaction.execute(&query).await?.rows_affected)
}

pub(crate) async fn fetch_optional<O, Q>(
    transaction: &PostgresTransaction,
    query: Q,
) -> Result<Option<O>, PostgresPersistenceError>
where
    O: FromRow,
    Q: Query<Output = O>,
{
    let rows = fetch_all(transaction, query).await?;
    if rows.len() > 1 {
        return Err(PostgresPersistenceError::Cardinality { actual: rows.len() });
    }
    Ok(rows.into_iter().next())
}

pub(crate) async fn fetch_all<O, Q>(
    transaction: &PostgresTransaction,
    query: Q,
) -> Result<Vec<O>, PostgresPersistenceError>
where
    O: FromRow,
    Q: Query<Output = O>,
{
    let query = query.compile(&PostgresDialect)?;
    transaction
        .fetch_all(&query)
        .await?
        .rows
        .iter()
        .map(O::from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub(crate) async fn lock_idempotency_key(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock(idempotency.scope.as_str(), idempotency.key.as_str())
        .await?;
    Ok(())
}

pub(crate) async fn lock_node_placement(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    node_id: NodeId,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock(
            "a3s.cloud.node-placement",
            &format!("{}:{}", organization_id.as_uuid(), node_id.as_uuid()),
        )
        .await?;
    Ok(())
}

pub(crate) async fn idempotency_replay<T>(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotentWrite<T>>, PostgresPersistenceError>
where
    T: DeserializeOwned,
{
    lock_idempotency_key(transaction, idempotency).await?;
    let existing = fetch_optional::<(String, serde_json::Value), _>(
        transaction,
        select_from::<IdempotencyRecords>()
            .select((
                IdempotencyRecords::request_digest(),
                IdempotencyRecords::response(),
            ))
            .filter(IdempotencyRecords::scope_key().eq(idempotency.scope.as_str()))
            .filter(IdempotencyRecords::idempotency_key().eq(idempotency.key.as_str())),
    )
    .await?;
    let Some((request_digest, response)) = existing else {
        return Ok(None);
    };
    if request_digest != idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict.into());
    }
    Ok(Some(IdempotentWrite {
        value: serde_json::from_value(response)?,
        replayed: true,
    }))
}

pub(crate) async fn store_idempotency<T>(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
    response: &T,
) -> Result<(), PostgresPersistenceError>
where
    T: Serialize,
{
    let rows = execute(
        transaction,
        insert_into::<IdempotencyRecords>()
            .value(IdempotencyRecords::scope_key(), idempotency.scope.as_str())
            .value(
                IdempotencyRecords::idempotency_key(),
                idempotency.key.as_str(),
            )
            .value(
                IdempotencyRecords::request_digest(),
                idempotency.request_digest.as_str(),
            )
            .value(
                IdempotencyRecords::response(),
                serde_json::to_value(response)?,
            )
            .value(IdempotencyRecords::created_at(), Utc::now()),
    )
    .await?;
    require_one_row("idempotency record", rows)
}

pub(crate) async fn store_outbox(
    transaction: &PostgresTransaction,
    event: &DomainEventEnvelope,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        insert_into::<OutboxEvents>()
            .value(OutboxEvents::event_id(), event.event_id)
            .value(OutboxEvents::event_key(), event.event_key.as_str())
            .value(OutboxEvents::schema_version(), event.schema_version)
            .value(OutboxEvents::organization_id(), event.organization_id)
            .value(OutboxEvents::aggregate_id(), event.aggregate_id)
            .value(OutboxEvents::aggregate_version(), event.aggregate_version)
            .value(OutboxEvents::occurred_at(), event.occurred_at)
            .value(OutboxEvents::correlation_id(), event.correlation_id)
            .value(OutboxEvents::causation_id(), event.causation_id)
            .value(OutboxEvents::payload(), event.payload.clone()),
    )
    .await?;
    require_one_row("outbox event", rows)
}

pub(crate) async fn store_audit(
    transaction: &PostgresTransaction,
    audit: &AuditWrite,
) -> Result<(), PostgresPersistenceError> {
    if audit.audit_id.is_nil()
        || audit.organization_id.is_nil()
        || audit.aggregate_id.is_nil()
        || audit.request_id.is_nil()
        || crate::modules::shared_kernel::domain::validate_audit_action(audit.action).is_err()
        || !audit.details.is_object()
    {
        return Err(PostgresPersistenceError::Invariant(
            "audit record is invalid".into(),
        ));
    }
    require_one_row(
        "audit record",
        execute(
            transaction,
            insert_into::<AuditRecords>()
                .value(AuditRecords::audit_id(), audit.audit_id)
                .value(AuditRecords::organization_id(), audit.organization_id)
                .value(AuditRecords::actor_id(), audit.actor_id)
                .value(AuditRecords::action(), audit.action)
                .value(AuditRecords::aggregate_id(), audit.aggregate_id)
                .value(AuditRecords::occurred_at(), audit.occurred_at)
                .value(AuditRecords::request_id(), audit.request_id)
                .value(AuditRecords::details(), audit.details.clone()),
        )
        .await?,
    )
}

pub(crate) fn require_one_row(
    resource: &str,
    rows_affected: u64,
) -> Result<(), PostgresPersistenceError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(PostgresPersistenceError::Invariant(format!(
            "writing {resource} affected {rows_affected} rows"
        )))
    }
}

pub(crate) fn is_unique_violation(error: &PostgresPersistenceError) -> bool {
    database_error_code(error) == Some("23505")
}

pub(crate) fn is_foreign_key_violation(error: &PostgresPersistenceError) -> bool {
    database_error_code(error) == Some("23503")
}

fn database_error_code(error: &PostgresPersistenceError) -> Option<&str> {
    let PostgresPersistenceError::Database(PostgresError::Database(error)) = error else {
        return None;
    };
    error.code().map(|code| code.code())
}

pub(crate) fn transaction_error(
    error: PostgresTransactionError<PostgresPersistenceError>,
) -> RepositoryError {
    match error {
        PostgresTransactionError::Operation(error) => error.into_repository(),
        PostgresTransactionError::Begin(error) => {
            RepositoryError::Storage(format!("could not begin PostgreSQL transaction: {error}"))
        }
        PostgresTransactionError::Commit(error) => {
            RepositoryError::Storage(format!("could not commit PostgreSQL transaction: {error}"))
        }
        PostgresTransactionError::OperationAndRollback {
            operation,
            rollback,
        } => RepositoryError::Storage(format!(
            "PostgreSQL operation failed ({operation}) and rollback failed ({rollback})"
        )),
    }
}

#[cfg(test)]
mod workflow_semantic_contract_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/103_workflow_semantic_contracts.sql"
    ));

    #[test]
    fn migration_103_keeps_v1_and_pairs_plan_v2_with_three_immutable_contracts() {
        for expected in [
            "compiler_schema_version in (1, 2)",
            "cloud.workflow.plan.v1",
            "cloud.workflow.plan-compiler.v1",
            "cloud.workflow.plan.v2",
            "cloud.workflow.plan-compiler.v2",
            "descriptor_bindings",
            "descriptor_registry",
            "variable_contract",
            "contract_count <> 3",
            "cannot downgrade compiler schema authority",
            "deferrable initially deferred",
            "workflow_revision_semantic_contracts_immutable",
            "reject_workflow_immutable_mutation()",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "missing migration guard {expected}"
            );
        }
    }
}

#[cfg(test)]
mod project_attribution_migration_tests {
    #[test]
    fn migration_104_adds_bounded_immutable_project_attribution_history() {
        const SQL: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/104_project_attribution_profiles.sql"
        ));

        for required in [
            "create table project_attribution_profiles",
            "previous_profile_id uuid",
            "add column current_attribution_profile_id uuid",
            "projects_current_attribution_profile_fk",
            "project_attribution_profiles_immutable",
            "project_attribution_profiles_validate_lineage",
            "projects_validate_attribution_pointer",
            "before update or delete on project_attribution_profiles",
            "references identity_principals(id)",
            "project_attribution_labels_are_valid(labels)",
            "label_key !~ '^[a-z][a-z0-9._-]{0,62}$'",
            "Immutable non-monetary showback metadata revisions",
        ] {
            assert!(
                SQL.contains(required),
                "migration 104 is missing {required}"
            );
        }
        for forbidden in [
            "create table invoices",
            "create table billing_accounts",
            "create table credit_ledger",
            "create table prices",
        ] {
            assert!(
                !SQL.to_ascii_lowercase().contains(forbidden),
                "migration 104 must not introduce monetary authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod workflow_run_input_v2_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/105_workflow_run_input_v2.sql"
    ));

    #[test]
    fn migration_105_widens_the_existing_immutable_run_input_without_new_storage() {
        for expected in [
            "drop constraint workflow_runs_execution_input_check",
            "add constraint workflow_runs_execution_input_check",
            "octet_length(execution_input) between 1 and 33554432",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "missing migration guard {expected}"
            );
        }
        assert!(!MIGRATION.contains("create table"));
    }
}

#[cfg(test)]
mod workflow_variable_defaults_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/107_workflow_variable_defaults.sql"
    ));

    #[test]
    fn migration_107_adds_only_immutable_revision_material_and_run_input_capacity() {
        for expected in [
            "'variable_defaults'",
            "cloud.workflow.variable-defaults.v1",
            "contract_count not in (3, 4)",
            "octet_length(execution_input) between 1 and 37748736",
            "not a mutable node catalog or variable store",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 107 is missing {expected}"
            );
        }
        for forbidden in [
            "create table workflow_run_variables",
            "create table workflow_variable_values",
            "create table workflow_variable_events",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 107 must not add a variable state mechanism: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod workflow_composite_regions_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/108_workflow_composite_regions.sql"
    ));

    #[test]
    fn migration_108_adds_only_immutable_region_policy_and_run_input_capacity() {
        assert_eq!(
            crate::modules::workflow::WORKFLOW_RUN_INPUT_MAX_BYTES_V2,
            38_797_312
        );
        for expected in [
            "'composite_regions'",
            "cloud.workflow.composite-regions.v1",
            "required_contract_count <> 3",
            "contract_count not in (3, 4, 5)",
            "octet_length(execution_input) between 1 and 38797312",
            "not a mutable node catalog, variable store, scheduler, or queue",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 108 is missing {expected}"
            );
        }
        for forbidden in [
            "create table workflow_composite",
            "create table workflow_iteration",
            "create table workflow_loop",
            "create table workflow_region_events",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 108 must not add a composite execution mechanism: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_inbox_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/106_notification_inbox.sql"
    ));

    #[test]
    fn migration_106_is_a_deduplicated_outbox_projection_not_a_second_queue() {
        for expected in [
            "create table notifications",
            "unique (source_event_id, recipient_principal_id)",
            "references outbox_events(event_id)",
            "notifications_recipient_feed_idx",
            "notifications_recipient_unread_idx",
            "validate_notification_source_event",
            "notifications_read_transition_only",
            "Notifications cannot be deleted",
            "aggregate_version = 1 and read_at is null",
            "aggregate_version = 2 and read_at is not null",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 106 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "create table notification_queue",
            "create table notification_providers",
            "create table notification_templates",
            "create table notification_subscriptions",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 106 must reuse the transactional outbox and existing ACL authority: {forbidden}"
            );
        }
    }
}
