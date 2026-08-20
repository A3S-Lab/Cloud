use super::flow::{scoped_postgres_url, BOOT_SCHEMA, FLOW_SCHEMA};
use super::postgres_access::{
    prepare_postgres_serving_access, reconcile_postgres_serving_access, PostgresServingAccessError,
};
use super::postgres_schema::{AuditRecords, IdempotencyRecords, OutboxEvents};
use crate::config::valid_postgres_role_name;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NodeId, OrganizationId, RepositoryError,
};
use a3s_boot::{migrate_postgres_queue, BootError, HealthIndicatorResult};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_flow::{migrate_postgres_flow, FlowError};
use a3s_orm::migration::MigrationRunError;
use a3s_orm::{
    insert_into, select_from, DecodeError, Executor, FromRow, Migration, Migrator, PostgresDialect,
    PostgresError, PostgresExecutor, PostgresMigrationError, PostgresTransaction,
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
    #[error(
        "PostgreSQL schema admission failed: {0}; run a3s-cloud-migrate before starting a serving process"
    )]
    Admission(#[source] MigrationRunError<PostgresMigrationError>),
    #[error("could not configure a component PostgreSQL schema: {0}")]
    ComponentConfiguration(String),
    #[error("could not migrate the A3S Flow PostgreSQL schema: {0}")]
    FlowMigration(#[source] FlowError),
    #[error("could not migrate the A3S Boot PostgreSQL schema: {0}")]
    BootMigration(#[source] BootError),
    #[error(
        "invalid PostgreSQL serving role {0:?}; expected a lowercase identifier up to 63 bytes without a reserved role name"
    )]
    InvalidServingRole(String),
    #[error("PostgreSQL serving role {0:?} does not exist")]
    ServingRoleMissing(String),
    #[error("PostgreSQL serving role {0:?} is also the migration role")]
    ServingRoleMatchesMigration(String),
    #[error("PostgreSQL serving role {0:?} is a member of the migration role")]
    ServingRoleInheritsMigration(String),
    #[error("PostgreSQL serving role {0:?} has administrative role attributes")]
    ServingRoleIsPrivileged(String),
    #[error("could not reconcile PostgreSQL serving access: {0}")]
    ServingAccess(#[source] PostgresError),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PostgresMigrationReport {
    pub applied: Vec<String>,
}

impl PostgresMigrationReport {
    pub fn is_up_to_date(&self) -> bool {
        self.applied.is_empty()
    }
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

pub async fn connect_postgres(
    url: &str,
    max_connections: usize,
) -> Result<PostgresExecutor, PostgresBootstrapError> {
    let executor = PostgresExecutor::connect_no_tls(url, max_connections)?;
    verify_postgres(&executor).await?;
    Ok(executor)
}

pub async fn migrate_postgres(
    url: &str,
    max_connections: usize,
    serving_role: &str,
) -> Result<PostgresMigrationReport, PostgresBootstrapError> {
    if !valid_postgres_role_name(serving_role) {
        return Err(PostgresBootstrapError::InvalidServingRole(
            serving_role.to_owned(),
        ));
    }
    let executor = PostgresExecutor::connect_no_tls(url, max_connections)?;
    let serving_access = prepare_postgres_serving_access(&executor, serving_role)
        .await
        .map_err(|error| match error {
            PostgresServingAccessError::MissingRole => {
                PostgresBootstrapError::ServingRoleMissing(serving_role.to_owned())
            }
            PostgresServingAccessError::MigrationRoleCollision => {
                PostgresBootstrapError::ServingRoleMatchesMigration(serving_role.to_owned())
            }
            PostgresServingAccessError::MigrationRoleMembership => {
                PostgresBootstrapError::ServingRoleInheritsMigration(serving_role.to_owned())
            }
            PostgresServingAccessError::PrivilegedRole => {
                PostgresBootstrapError::ServingRoleIsPrivileged(serving_role.to_owned())
            }
            PostgresServingAccessError::Database(error) => {
                PostgresBootstrapError::ServingAccess(error)
            }
        })?;
    let cloud_report = Migrator::new(executor.clone())
        .run(cloud_migrations())
        .await?;
    verify_postgres(&executor).await?;

    let flow_url = scoped_postgres_url(url, FLOW_SCHEMA)
        .map_err(|error| PostgresBootstrapError::ComponentConfiguration(error.to_string()))?;
    let flow_executor = PostgresExecutor::connect_no_tls(flow_url.as_str(), max_connections)?;
    let flow_report = migrate_postgres_flow(&flow_executor)
        .await
        .map_err(PostgresBootstrapError::FlowMigration)?;

    let boot_url = scoped_postgres_url(url, BOOT_SCHEMA)
        .map_err(|error| PostgresBootstrapError::ComponentConfiguration(error.to_string()))?;
    let boot_executor = PostgresExecutor::connect_no_tls(boot_url.as_str(), max_connections)?;
    let boot_report = migrate_postgres_queue(&boot_executor)
        .await
        .map_err(PostgresBootstrapError::BootMigration)?;
    reconcile_postgres_serving_access(&executor, &serving_access)
        .await
        .map_err(PostgresBootstrapError::ServingAccess)?;

    let mut applied = cloud_report.applied;
    applied.extend(
        flow_report
            .applied
            .into_iter()
            .map(|version| format!("{FLOW_SCHEMA}/{version}")),
    );
    applied.extend(
        boot_report
            .applied
            .into_iter()
            .map(|version| format!("{BOOT_SCHEMA}/{version}")),
    );
    Ok(PostgresMigrationReport { applied })
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
        Migration::new(
            "109",
            "immutable Connector profiles and Secret bindings",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/109_connector_profiles.sql"
            )),
        ),
        Migration::new(
            "110",
            "race-safe active Connector Secret admission",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/110_connector_active_secret_admission.sql"
            )),
        ),
        Migration::new(
            "111",
            "typed Connector Secret admission failures",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/111_connector_secret_admission_error.sql"
            )),
        ),
        Migration::new(
            "112",
            "immutable Connector execution evidence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/112_connector_execution_evidence.sql"
            )),
        ),
        Migration::new(
            "113",
            "fenced Connector execution attempts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/113_connector_execution_attempts.sql"
            )),
        ),
        Migration::new(
            "114",
            "outbound notification subscriptions and receipts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/114_notification_outbound_delivery.sql"
            )),
        ),
        Migration::new(
            "115",
            "bounded outbound notification attempt receipts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/115_notification_outbound_attempt_budget.sql"
            )),
        ),
        Migration::new(
            "116",
            "immutable Durable Cell applications and revisions",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/116_durable_cell_applications.sql"
            )),
        ),
        Migration::new(
            "117",
            "immutable Durable Cell deployment correlations",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/117_durable_cell_deployments.sql"
            )),
        ),
        Migration::new(
            "118",
            "typed BuildRun published outputs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/118_typed_build_outputs.sql"
            )),
        ),
        Migration::new(
            "119",
            "node-bound internal Execution Tasks",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/119_bound_execution_tasks.sql"
            )),
        ),
        Migration::new(
            "120",
            "exact Durable Cell S0 provider profiles",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/120_durable_cell_provider_profiles.sql"
            )),
        ),
        Migration::new(
            "121",
            "deployment infrastructure bindings",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/121_infrastructure_bindings.sql"
            )),
        ),
        Migration::new(
            "122",
            "Workflow step default-output evidence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/122_workflow_step_default_output_evidence.sql"
            )),
        ),
    ]
}

async fn verify_postgres(executor: &PostgresExecutor) -> Result<(), PostgresBootstrapError> {
    Migrator::new(executor.clone())
        .verify_required(cloud_migrations())
        .await
        .map_err(PostgresBootstrapError::Admission)
}

pub async fn postgres_health(executor: PostgresExecutor) -> HealthIndicatorResult {
    match verify_postgres(&executor).await {
        Ok(_) => HealthIndicatorResult::up(),
        Err(error) => HealthIndicatorResult::down().with_detail_value("error", error.to_string()),
    }
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
mod connector_profile_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/109_connector_profiles.sql"
    ));

    #[test]
    fn migration_109_adds_one_immutable_connector_authority_without_execution_mechanisms() {
        for expected in [
            "create table connector_profiles",
            "create table connector_revisions",
            "create table connector_revision_secret_bindings",
            "cloud.connector.http.v1",
            "references secret_versions (secret_id, version)",
            "revision.created_at = new.updated_at",
            "Connector revisions are immutable",
            "not an execution queue, scheduler, retry store, or Secret authority",
            "never plaintext or copied Secret state",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 109 is missing {expected}"
            );
        }
        for forbidden in [
            "create table connector_jobs",
            "create table connector_attempts",
            "create table connector_retries",
            "create table connector_secret_material",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 109 must not add a duplicate execution or Secret mechanism: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod connector_active_secret_admission_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/110_connector_active_secret_admission.sql"
    ));

    #[test]
    fn migration_110_fences_active_secret_admission_without_owning_secret_lifecycle() {
        for expected in [
            "before insert on connector_revision_secret_bindings",
            "s.state = 'active'",
            "v.state = 'active'",
            "for share of s, v",
            "Secrets remains lifecycle authority",
            "execution must recheck just in time",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 110 is missing {expected}"
            );
        }
        for forbidden in [
            "before update on secrets",
            "before update on secret_versions",
            "create table",
            "create queue",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 110 must not become another Secret or execution authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod connector_secret_admission_error_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/111_connector_secret_admission_error.sql"
    ));

    #[test]
    fn migration_111_preserves_the_row_fence_and_restores_foreign_key_semantics() {
        for expected in [
            "create or replace function validate_connector_secret_binding_materializable()",
            "for share of s, v",
            "raise foreign_key_violation",
            "Secrets remains lifecycle authority",
            "execution must recheck just in time",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 111 is missing {expected}"
            );
        }
        for forbidden in ["create table", "create trigger", "create queue"] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 111 must only normalize the existing admission constraint: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod connector_execution_evidence_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/112_connector_execution_evidence.sql"
    ));

    #[test]
    fn migration_112_adds_only_bounded_immutable_terminal_facts() {
        for expected in [
            "create table connector_execution_evidence",
            "references connector_revisions",
            "request_body_bytes between 0 and 1048576",
            "response_body_bytes between 0 and 1048576",
            "retry_after_seconds between 0 and 86400",
            "Connector execution evidence is immutable",
            "not an execution queue, attempt reservation, retry store, scheduler",
            "request headers, bodies, signing input, endpoints, addresses, and credentials are never stored",
            "provider response bytes and text are never stored",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 112 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "create table connector_jobs",
            "create table connector_attempts",
            "create table connector_retries",
            "create table connector_request_bodies",
            "create table connector_response_bodies",
            "create table connector_credentials",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 112 must not add another execution mechanism: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod connector_execution_attempt_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/113_connector_execution_attempts.sql"
    ));

    #[test]
    fn migration_113_adds_one_fenced_attempt_boundary_without_retry_authority() {
        for expected in [
            "create table connector_execution_attempts",
            "state in ('reserved', 'dispatching', 'terminal')",
            "lease_expires_at <= reserved_at + interval '30 seconds'",
            "outcome_deadline_at <= dispatch_started_at + interval '120 seconds'",
            "Connector execution reservation takeover is not fenced",
            "Terminal Connector execution attempt requires exact evidence",
            "Connector execution evidence requires its exact terminal attempt",
            "paired_dispatch_started_at is null",
            "new.outcome = 'accepted'",
            "dispatching is never reacquired",
            "not a queue, retry schedule, Flow history, provider receipt store, or acknowledgement authority",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 113 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "attempt_count",
            "retry_count",
            "next_attempt_at",
            "available_at",
            "create table connector_queue",
            "create table connector_retries",
            "create table connector_responses",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 113 must not add retry, queue, or response authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_outbound_delivery_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/114_notification_outbound_delivery.sql"
    ));

    #[test]
    fn migration_114_adds_acl_subscriptions_and_receipts_without_another_retry_rail() {
        for expected in [
            "create table notification_outbound_subscriptions",
            "cloud.notification.outbound-subscription.v1",
            "references connector_revisions",
            "notification_outbound_subscription_revoke_only",
            "create table notification_outbound_deliveries",
            "references outbox_events(event_id)",
            "notification.delivery.requested",
            "notification_outbound_delivery_validate_fact",
            "notification_outbound_delivery_terminal_only",
            "notification_outbound_delivery_terminal_receipt_exact",
            "does not match its exact C6 attempt",
            "not a queue, retry schedule, retry counter",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 114 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "retry_count",
            "next_attempt_at",
            "available_at",
            "create table notification_queue",
            "create table notification_retries",
            "provider_response",
            "response_body",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 114 must not add queue, retry, or provider-response authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_outbound_attempt_budget_migration_tests {
    use crate::modules::notifications::MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS;

    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/115_notification_outbound_attempt_budget.sql"
    ));

    #[test]
    fn migration_115_terminates_retryable_evidence_without_another_retry_mechanism() {
        for expected in [
            "'exhausted'",
            "evidence_outcome is distinct from 'retryable'",
            "exact C6 attempt and delivery budget",
            "create or replace function validate_notification_outbound_terminal_receipt",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 115 is missing {expected}"
            );
        }
        assert!(
            MIGRATION.contains(&format!(
                "new.terminal_generation is distinct from {MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS}"
            )),
            "migration 115 must match the domain provider-attempt budget"
        );
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "create table",
            "add column",
            "retry_count",
            "next_attempt",
            "next_retry",
            "token_bucket",
            "rate_bucket",
            "provider_response",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 115 must not add another rate/retry authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod durable_cell_application_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/116_durable_cell_applications.sql"
    ));

    #[test]
    fn migration_116_persists_only_the_revisioned_application_authority() {
        for expected in [
            "create table durable_cell_applications",
            "create table durable_cell_application_revisions",
            "cloud.durable-cell.application.v1",
            "references build_runs",
            "deferrable initially deferred",
            "validate_durable_cell_revision_lineage",
            "reject_durable_cell_revision_mutation",
            "validate_durable_cell_application_update",
            "Durable Cell desired-state update changed revision authority or was a no-op",
            "not a deployment pointer or provider receipt",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 116 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "create table cells ",
            "create table durable_cell_deployments",
            "create table cell_ownership",
            "create table cell_state",
            "create table durable_cell_queue",
            "create table durable_cell_scheduler",
            "create table durable_cell_provider_receipts",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 116 must not add per-Cell, deployment, queue, scheduler, or provider-receipt authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod durable_cell_deployment_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/117_durable_cell_deployments.sql"
    ));

    #[test]
    fn migration_117_persists_only_immutable_existing_owner_correlation() {
        for expected in [
            "create table durable_cell_deployments",
            "references durable_cell_application_revisions",
            "reject_durable_cell_deployment_mutation",
            "Workloads owns Deployment",
            "Operations owns execution",
            "Fleet owns receipts",
            "S0 owns namespace behavior",
            "not a second deployment authority",
            "not a namespace lifecycle record",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 117 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "deployment_status",
            "rollout_status",
            "retry_count",
            "next_attempt",
            "provider_receipt",
            "cell_name",
            "create table durable_cell_operations",
            "create table durable_cell_commands",
            "create table durable_cell_namespaces",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 117 added another lifecycle authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod typed_build_output_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/118_typed_build_outputs.sql"
    ));

    #[test]
    fn migration_118_extends_the_existing_build_authority_only() {
        for expected in [
            "add column published_output jsonb",
            "application/vnd.a3s.durable-cell.bundle.v1+tar",
            "a3s-cloud-artifact://sha256/",
            "published_output - array['uri', 'digest', 'mediaType', 'sizeBytes']",
            "published_output ->> 'digest' <> published_artifact ->> 'digest'",
            "jsonb_array_length(evidence #> '{provenance,subject}') = 3",
            "internalParameters,publishedOutput}'",
            "not an OCI manifest alias or a Durable Cells lifecycle authority",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 118 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "create table",
            "create index",
            "bundle_cache",
            "bundle_publisher",
            "bundle_downloader",
            "durable_cell_build",
            "jsonb_object_length",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 118 added another build, artifact, or Durable Cells authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod bound_execution_task_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/119_bound_execution_tasks.sql"
    ));

    #[test]
    fn migration_119_extends_the_existing_execution_authority_only() {
        for expected in [
            "alter table executions",
            "add column target_node_id uuid",
            "add column task_policy jsonb",
            "check (coalesce((",
            "task_policy - array[",
            "jsonb_array_length(task_policy -> 'mounts') between 1 and 128",
            "jsonb_array_length(task_policy -> 'secrets') between 1 and 128",
            "references nodes (organization_id, id)",
            "not product configuration or another task lifecycle",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 119 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "create table",
            "create index",
            "durable_cell_task",
            "publisher_queue",
            "publisher_worker",
            "secret_value",
            "retry_count",
            "jsonb_object_length",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 119 added another execution, publisher, or Secret authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod durable_cell_provider_profile_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/120_durable_cell_provider_profiles.sql"
    ));

    #[test]
    fn migration_120_extends_only_the_existing_correlation() {
        for expected in [
            "alter table durable_cell_deployments",
            "add column storage_provider_profile_acl text",
            "octet_length(storage_provider_profile_acl) between 1 and 16384",
            "without CELL0.5-C3b''s backwards-compatible optional profile input",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 120 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "create table",
            "create index",
            "publisher_queue",
            "publisher_state",
            "secret_value",
            "retry_count",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 120 added another publication or Secret authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod infrastructure_binding_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/121_infrastructure_bindings.sql"
    ));

    #[test]
    fn migration_121_adds_one_generic_create_only_topology_authority() {
        for expected in [
            "create table infrastructure_bindings",
            "binding_name varchar(128) primary key",
            "binding_schema varchar(128) not null",
            "binding_digest char(71) not null",
            "replacement requires an explicit migration",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "migration 121 is missing {expected}"
            );
        }
        let lower = MIGRATION.to_ascii_lowercase();
        for forbidden in [
            "secret_value",
            "credential_value",
            "object_body",
            "git_ref",
            "updated_at",
            "on update",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 121 added mutable or duplicated authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod workflow_default_output_evidence_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/122_workflow_step_default_output_evidence.sql"
    ));

    #[test]
    fn migration_122_adds_nullable_authority_bound_fallback_evidence() {
        for expected in [
            "pg_get_constraintdef(constraint_record.oid) like '%kind%branch%selected_handle%'",
            "workflow_step_projections_selected_handle_routing_check",
            "kind = 'execution'",
            "status = 'failed'",
            "add column default_output_evidence jsonb",
            "cloud.workflow.step-default-output.v1",
            "default_output_evidence #>> '{failure,stepId}' = step_id",
            "status = 'completed'",
            "selected_handle is null",
            "error is null",
            ") is true",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        assert!(!MIGRATION
            .contains("add constraint workflow_step_projections_selected_handle_check check"));
        assert!(!MIGRATION.contains("create table"));
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
