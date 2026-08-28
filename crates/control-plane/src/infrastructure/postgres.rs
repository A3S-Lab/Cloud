use super::flow::{scoped_postgres_url, BOOT_SCHEMA, FLOW_SCHEMA};
use super::postgres_access::{
    prepare_postgres_serving_access, reconcile_postgres_serving_access, PostgresServingAccessError,
};
use super::postgres_schema::{AuditRecords, IdempotencyRecords, OutboxEvents};
use crate::config::valid_postgres_role_name;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, InstallationId, NodeId, OrganizationId,
    ProjectId, RepositoryError, ScopeContext,
};
use a3s_boot::{migrate_postgres_queue, BootError, HealthIndicatorResult};
use a3s_cloud_contracts::{CloudScopeRef, DomainEventEnvelope};
use a3s_flow::{migrate_postgres_flow, FlowError};
use a3s_orm::migration::MigrationRunError;
use a3s_orm::{
    insert_into, select_from, sql_query, DecodeError, Executor, FromRow, Migration, Migrator,
    PostgresDialect, PostgresError, PostgresExecutor, PostgresMigrationError, PostgresTransaction,
    PostgresTransactionError, Query,
};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

pub(crate) struct AuditWrite {
    pub(crate) audit_id: Uuid,
    pub(crate) scope: CloudScopeRef,
    pub(crate) actor_id: Option<Uuid>,
    pub(crate) action: &'static str,
    pub(crate) aggregate_id: Uuid,
    pub(crate) occurred_at: DateTime<Utc>,
    pub(crate) request_id: Uuid,
    pub(crate) details: serde_json::Value,
}

impl AuditWrite {
    pub(crate) const fn organization_scope(organization_id: Uuid) -> CloudScopeRef {
        CloudScopeRef::Organization { organization_id }
    }

    pub(crate) const fn resource_scope(
        organization_id: Uuid,
        project_id: ProjectId,
        environment_id: Option<EnvironmentId>,
    ) -> CloudScopeRef {
        match environment_id {
            Some(environment_id) => CloudScopeRef::Environment {
                organization_id,
                project_id: project_id.as_uuid(),
                environment_id: environment_id.as_uuid(),
            },
            None => CloudScopeRef::Project {
                organization_id,
                project_id: project_id.as_uuid(),
            },
        }
    }
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

pub const CLOUD_MIGRATION_COUNT: i64 = 176;
pub const LATEST_CLOUD_MIGRATION_VERSION: &str = "176";

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
        Migration::new(
            "123",
            "Workflow Connector step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/123_workflow_connector_step_projections.sql"
            )),
        ),
        Migration::new(
            "124",
            "immutable Application releases",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/124_application_releases.sql"
            )),
        ),
        Migration::new(
            "125",
            "Application sessions and semantic effects",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/125_application_sessions.sql"
            )),
        ),
        Migration::new(
            "126",
            "Application invocation Workflow authority",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/126_application_invocation_workflow_authority.sql"
            )),
        ),
        Migration::new(
            "127",
            "Application invocation timeout policy",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/127_application_invocation_timeout_policy.sql"
            )),
        ),
        Migration::new(
            "128",
            "versioned outbound notification delivery budgets",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/128_notification_outbound_delivery_budget.sql"
            )),
        ),
        Migration::new(
            "129",
            "bounded outbound notification event-time suppression",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/129_notification_outbound_suppression.sql"
            )),
        ),
        Migration::new(
            "130",
            "immutable personal notification alert policies",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/130_notification_alert_policies.sql"
            )),
        ),
        Migration::new(
            "131",
            "immutable Workload writer-fence receipts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/131_workload_writer_fence_receipts.sql"
            )),
        ),
        Migration::new(
            "132",
            "durable Agent Code commands",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/132_agent_code_command_persistence.sql"
            )),
        ),
        Migration::new(
            "133",
            "Gateway certificate-renewal notification alert source",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/133_notification_alert_policy_certificate_source.sql"
            )),
        ),
        Migration::new(
            "134",
            "Workload deployment-health notification alert source",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/134_notification_alert_policy_workload_source.sql"
            )),
        ),
        Migration::new(
            "135",
            "Gateway certificate-expiry notification alert source",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/135_notification_alert_policy_certificate_expiry_source.sql"
            )),
        ),
        Migration::new(
            "136",
            "Identity verified recipient contacts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/136_identity_recipient_contacts.sql"
            )),
        ),
        Migration::new(
            "137",
            "Identity recipient-contact verification delivery",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/137_identity_recipient_contact_verification_delivery.sql"
            )),
        ),
        Migration::new(
            "138",
            "Notifications verified-contact SMTP delivery",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/138_notification_outbound_smtp.sql"
            )),
        ),
        Migration::new(
            "139",
            "Fleet node availability owner facts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/139_fleet_node_availability_facts.sql"
            )),
        ),
        Migration::new(
            "140",
            "notification alert policy Node targets",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/140_notification_alert_policy_node_target.sql"
            )),
        ),
        Migration::new(
            "141",
            "Security Gateway Route policy investigation timeline indexes",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/141_security_gateway_route_policy_timeline.sql"
            )),
        ),
        Migration::new(
            "142",
            "request-time audit attribution snapshots",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/142_audit_attribution_snapshots.sql"
            )),
        ),
        Migration::new(
            "143",
            "Workflow Application Answer step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/143_workflow_application_answer_step_projections.sql"
            )),
        ),
        Migration::new(
            "144",
            "monotonic audit retention authority",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/144_audit_retention_authority.sql"
            )),
        ),
        Migration::new(
            "145",
            "Workflow Transform failure step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/145_workflow_transform_failure_step_projections.sql"
            )),
        ),
        Migration::new(
            "146",
            "immutable accepted developer BuildPlans",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/146_developer_build_plans.sql"
            )),
        ),
        Migration::new(
            "147",
            "immutable accepted developer workload profile revisions",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/147_developer_workload_profile_revisions.sql"
            )),
        ),
        Migration::new(
            "148",
            "Workflow composite failure step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/148_workflow_composite_failure_step_projections.sql"
            )),
        ),
        Migration::new(
            "149",
            "Workflow payload schema versions",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/149_workflow_payload_schema_versions.sql"
            )),
        ),
        Migration::new(
            "150",
            "hosted build bounded-context identity guards",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/150_hosted_build_context_boundary.sql"
            )),
        ),
        Migration::new(
            "151",
            "Workflow List Operator payload schema",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/151_workflow_list_operator_payload_schema.sql"
            )),
        ),
        Migration::new(
            "152",
            "Artifacts-owned build candidate fact projection",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/152_artifact_build_candidate_projection.sql"
            )),
        ),
        Migration::new(
            "153",
            "immutable pull-request Preview policy revisions",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/153_developer_pull_request_preview_policy_revisions.sql"
            )),
        ),
        Migration::new(
            "154",
            "exact Connector revision revocation authority",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/154_connector_revision_revocations.sql"
            )),
        ),
        Migration::new(
            "155",
            "indeterminate Connector execution attempt resolution authority",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/155_connector_execution_attempt_resolutions.sql"
            )),
        ),
        Migration::new(
            "156",
            "typed pull-request webhook facts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/156_source_pull_request_webhook_facts.sql"
            )),
        ),
        Migration::new(
            "157",
            "Developer Workflows pull-request Preview projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/157_developer_pull_request_preview_projections.sql"
            )),
        ),
        Migration::new(
            "158",
            "Workflow cancellation compensation policy schema",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/158_workflow_cancellation_compensation_policy.sql"
            )),
        ),
        Migration::new(
            "159",
            "Sources pull-request Preview SourceRevision projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/159_source_pull_request_preview_revision_projections.sql"
            )),
        ),
        Migration::new(
            "160",
            "immutable Agent provider profiles and provider-neutral execution bindings",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/160_agent_provider_profiles.sql"
            )),
        ),
        Migration::new(
            "161",
            "Workflow Agent step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/161_workflow_agent_step_projections.sql"
            )),
        ),
        Migration::new(
            "162",
            "Artifacts Preview build lifecycle projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/162_artifact_preview_build_lifecycle_projections.sql"
            )),
        ),
        Migration::new(
            "163",
            "Workflow Agent failure step projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/163_workflow_agent_failure_step_projections.sql"
            )),
        ),
        Migration::new(
            "164",
            "immutable Agent provider selection",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/164_agent_provider_selection.sql"
            )),
        ),
        Migration::new(
            "165",
            "immutable Agent Harness invocation profiles",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/165_agent_harness_invocation_profiles.sql"
            )),
        ),
        Migration::new(
            "166",
            "auditable Agent Tool events",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/166_agent_tool_events.sql"
            )),
        ),
        Migration::new(
            "167",
            "Agent approval checkpoints",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/167_agent_approval_checkpoints.sql"
            )),
        ),
        Migration::new(
            "168",
            "immutable Agent execution checkpoints and fork lineage",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/168_agent_execution_checkpoints.sql"
            )),
        ),
        Migration::new(
            "169",
            "Agent checkpoint object capture and cleanup leases",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/169_agent_checkpoint_object_leases.sql"
            )),
        ),
        Migration::new(
            "170",
            "UserFile lifecycle metadata and organization quota",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/170_user_files.sql"
            )),
        ),
        Migration::new(
            "171",
            "Workflow-owned Application invocation timeout policy",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/171_application_invocation_timeout_policy_owner.sql"
            )),
        ),
        Migration::new(
            "172",
            "Fleet node protocol session heads",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/172_node_protocol_session_heads.sql"
            )),
        ),
        Migration::new(
            "173",
            "Workflow-owned HumanTask submission evidence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/173_human_task_submission_owner.sql"
            )),
        ),
        Migration::new(
            "174",
            "canonical Cloud Installation and scoped facts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/174_installation_scoped_facts.sql"
            )),
        ),
        Migration::new(
            "175",
            "legacy scoped fact writer compatibility",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/175_legacy_scoped_fact_writer_compatibility.sql"
            )),
        ),
        Migration::new(
            "176",
            "historical fact scope lifecycle",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/176_historical_fact_scope_lifecycle.sql"
            )),
        ),
    ]
}

#[cfg(test)]
#[path = "postgres_tests/workflow_transform_failure_migration.rs"]
mod workflow_transform_failure_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/cloud_migration_manifest.rs"]
mod cloud_migration_manifest_tests;

#[cfg(test)]
#[path = "postgres_tests/installation_scoped_facts_migration.rs"]
mod installation_scoped_facts_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/node_protocol_session_migration.rs"]
mod node_protocol_session_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/workflow_composite_failure_migration.rs"]
mod workflow_composite_failure_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/workflow_payload_schema_versions_migration.rs"]
mod workflow_payload_schema_versions_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/hosted_build_context_boundary_migration.rs"]
mod hosted_build_context_boundary_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/workflow_list_operator_payload_schema_migration_151.rs"]
mod workflow_list_operator_payload_schema_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/workflow_cancellation_compensation_policy_migration.rs"]
mod workflow_cancellation_compensation_policy_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/artifact_build_candidate_projection_migration.rs"]
mod artifact_build_candidate_projection_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/connector_revision_revocation_migration.rs"]
mod connector_revision_revocation_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/connector_execution_attempt_resolution_migration.rs"]
mod connector_execution_attempt_resolution_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/source_pull_request_webhook_facts_migration.rs"]
mod source_pull_request_webhook_facts_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/developer_pull_request_preview_projection_migration.rs"]
mod developer_pull_request_preview_projection_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/source_pull_request_preview_revision_projection_migration.rs"]
mod source_pull_request_preview_revision_projection_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/agent_provider_profile_migration.rs"]
mod agent_provider_profile_migration_tests;

#[cfg(test)]
#[path = "postgres_tests/artifact_preview_build_lifecycle_projection_migration.rs"]
mod artifact_preview_build_lifecycle_projection_migration_tests;

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

async fn resolve_cloud_scope(
    transaction: &PostgresTransaction,
    reference: &CloudScopeRef,
) -> Result<ScopeContext, PostgresPersistenceError> {
    reference
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let installation_id = match *reference {
        CloudScopeRef::Installation { installation_id } => fetch_optional::<Uuid, _>(
            transaction,
            sql_query::<Uuid>(
                "select installation.id from cloud_installations installation where installation.singleton_key and installation.id = ",
            )
            .bind(installation_id)
            .append(" for share of installation"),
        )
        .await?,
        CloudScopeRef::Organization { organization_id } => fetch_optional::<Uuid, _>(
            transaction,
            sql_query::<Uuid>(
                "select organization.installation_id from organizations organization where organization.id = ",
            )
            .bind(organization_id)
            .append(" for share of organization"),
        )
        .await?,
        CloudScopeRef::Project {
            organization_id,
            project_id,
        } => fetch_optional::<Uuid, _>(
            transaction,
            sql_query::<Uuid>(
                "select organization.installation_id from organizations organization join projects project on project.organization_id = organization.id where organization.id = ",
            )
            .bind(organization_id)
            .append(" and project.id = ")
            .bind(project_id)
            .append(" for share of organization, project"),
        )
        .await?,
        CloudScopeRef::Environment {
            organization_id,
            project_id,
            environment_id,
        } => fetch_optional::<Uuid, _>(
            transaction,
            sql_query::<Uuid>(
                "select organization.installation_id from organizations organization join projects project on project.organization_id = organization.id join environments environment on environment.organization_id = project.organization_id and environment.project_id = project.id where organization.id = ",
            )
            .bind(organization_id)
            .append(" and project.id = ")
            .bind(project_id)
            .append(" and environment.id = ")
            .bind(environment_id)
            .append(" for share of organization, project, environment"),
        )
        .await?,
    }
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Cloud fact scope does not resolve to one canonical Installation lineage".into(),
        )
    })?;
    ScopeContext::from_resolved_reference(InstallationId::from_uuid(installation_id), *reference)
        .map_err(PostgresPersistenceError::Invariant)
}

pub(crate) async fn store_outbox(
    transaction: &PostgresTransaction,
    event: &DomainEventEnvelope,
) -> Result<(), PostgresPersistenceError> {
    event
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let scope = resolve_cloud_scope(transaction, &event.scope).await?;
    let rows = execute(
        transaction,
        insert_into::<OutboxEvents>()
            .value(OutboxEvents::event_id(), event.event_id)
            .value(OutboxEvents::event_key(), event.event_key.as_str())
            .value(OutboxEvents::schema_version(), event.schema_version)
            .value(
                OutboxEvents::installation_id(),
                scope.installation_id().as_uuid(),
            )
            .value(OutboxEvents::scope_kind(), scope.kind())
            .value(
                OutboxEvents::organization_id(),
                scope.organization_id().map(OrganizationId::as_uuid),
            )
            .value(
                OutboxEvents::project_id(),
                scope.project_id().map(ProjectId::as_uuid),
            )
            .value(
                OutboxEvents::environment_id(),
                scope.environment_id().map(EnvironmentId::as_uuid),
            )
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
        || audit.aggregate_id.is_nil()
        || audit.request_id.is_nil()
        || crate::modules::shared_kernel::domain::validate_audit_action(audit.action).is_err()
        || !audit.details.is_object()
    {
        return Err(PostgresPersistenceError::Invariant(
            "audit record is invalid".into(),
        ));
    }
    audit
        .scope
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let scope = resolve_cloud_scope(transaction, &audit.scope).await?;
    let (attribution_profile_id, attribution_status) =
        resolve_audit_attribution(transaction, audit, scope).await?;
    require_one_row(
        "audit record",
        execute(
            transaction,
            insert_into::<AuditRecords>()
                .value(AuditRecords::audit_id(), audit.audit_id)
                .value(
                    AuditRecords::installation_id(),
                    scope.installation_id().as_uuid(),
                )
                .value(AuditRecords::scope_kind(), scope.kind())
                .value(
                    AuditRecords::organization_id(),
                    scope.organization_id().map(OrganizationId::as_uuid),
                )
                .value(AuditRecords::actor_id(), audit.actor_id)
                .value(AuditRecords::action(), audit.action)
                .value(AuditRecords::aggregate_id(), audit.aggregate_id)
                .value(AuditRecords::occurred_at(), audit.occurred_at)
                .value(AuditRecords::request_id(), audit.request_id)
                .value(
                    AuditRecords::project_id(),
                    scope.project_id().map(ProjectId::as_uuid),
                )
                .value(
                    AuditRecords::environment_id(),
                    scope.environment_id().map(EnvironmentId::as_uuid),
                )
                .value(
                    AuditRecords::attribution_profile_id(),
                    attribution_profile_id,
                )
                .value(AuditRecords::attribution_status(), attribution_status)
                .value(AuditRecords::details(), audit.details.clone()),
        )
        .await?,
    )
}

async fn resolve_audit_attribution(
    transaction: &PostgresTransaction,
    audit: &AuditWrite,
    scope: ScopeContext,
) -> Result<(Option<Uuid>, &'static str), PostgresPersistenceError> {
    match scope {
        ScopeContext::Installation { .. } | ScopeContext::Organization { .. } => {
            Ok((None, "not_applicable"))
        }
        ScopeContext::Project {
            organization_id,
            project_id,
            ..
        }
        | ScopeContext::Environment {
            organization_id,
            project_id,
            ..
        } => {
            let profile = fetch_optional::<Uuid, _>(
                transaction,
                sql_query::<Uuid>(
                    "select profile.id from project_attribution_profiles profile where profile.organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and profile.project_id = ")
                .bind(project_id.as_uuid())
                .append(" and profile.created_at <= ")
                .bind(audit.occurred_at)
                .append(" order by profile.created_at desc, profile.id desc limit 1"),
            )
            .await?;
            Ok((
                profile,
                if profile.is_some() {
                    "profile_bound"
                } else {
                    "profile_missing"
                },
            ))
        }
    }
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
mod notification_outbound_delivery_budget_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/128_notification_outbound_delivery_budget.sql"
    ));

    #[test]
    fn migration_128_pins_versioned_budgets_without_another_delivery_mechanism() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "cloud.notification.outbound-subscription.v2",
            "add column maximum_provider_attempts",
            "subscription.maximum_provider_attempts <> new.maximum_provider_attempts",
            "requested.schema_version <> expected_schema_version",
            "new.maximum_provider_attempts is distinct from old.maximum_provider_attempts",
            "new.terminal_generation is distinct from new.maximum_provider_attempts",
            "pinned delivery budget",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "migration 128 is missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "retry_count",
            "next_attempt",
            "next_retry",
            "token_bucket",
            "rate_bucket",
            "provider_response",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 128 duplicated delivery authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_outbound_suppression_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/129_notification_outbound_suppression.sql"
    ));

    #[test]
    fn migration_129_filters_on_immutable_event_time_without_another_clock_or_queue() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "cloud.notification.outbound-subscription.v3",
            "add column suppress_before timestamptz",
            "suppress_before is not null",
            "suppress_before <= created_at + interval '30 days'",
            "inbox.occurred_at < subscription.suppress_before",
            "new.suppress_before is distinct from old.suppress_before",
            "when 'cloud.notification.outbound-subscription.v3' then 2",
            "event-time cutoff",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "migration 129 is missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "clock_timestamp",
            "now()",
            "pg_sleep",
            "retry_count",
            "suppression_count",
            "next_delivery",
            "token_bucket",
            "rate_bucket",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 129 duplicated suppression or delivery authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_alert_policy_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/130_notification_alert_policies.sql"
    ));

    #[test]
    fn migration_130_persists_only_immutable_closed_source_policies() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "create table notification_alert_policies",
            "cloud.notification.alert-policy.v1",
            "edge.domain-claim-status.v1",
            "notification_alert_policies_active_source_scope_idx",
            "new.notify_on_recovery is distinct from old.notify_on_recovery",
            "active-to-revoked transition",
            "compile-time closed owner-event registry",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "migration 130 is missing {expected}"
            );
        }
        for forbidden in [
            "json_path",
            "jsonpath",
            "metric_value",
            "incident_state",
            "firing_count",
            "clock_timestamp",
            "pg_sleep",
            "next_evaluation",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 130 duplicated alert evaluation authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_alert_policy_certificate_source_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/133_notification_alert_policy_certificate_source.sql"
    ));

    #[test]
    fn migration_133_only_widens_the_closed_alert_source_registry() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "drop constraint notification_alert_policies_source_check",
            "add constraint notification_alert_policies_source_check",
            "edge.domain-claim-status.v1",
            "edge.gateway-certificate-renewal-status.v1",
            "not valid",
            "validate constraint notification_alert_policies_source_check",
            "compile-time closed typed owner-event source registry",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "migration 133 is missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "create index",
            "create trigger",
            "json_path",
            "jsonpath",
            "metric_value",
            "incident_state",
            "firing_count",
            "clock_timestamp",
            "pg_sleep",
            "next_evaluation",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 133 duplicated alert authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_alert_policy_workload_source_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/134_notification_alert_policy_workload_source.sql"
    ));

    #[test]
    fn migration_134_only_widens_the_closed_alert_source_registry() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "drop constraint notification_alert_policies_source_check",
            "add constraint notification_alert_policies_source_check",
            "edge.domain-claim-status.v1",
            "edge.gateway-certificate-renewal-status.v1",
            "workload.deployment-health.v1",
            "not valid",
            "validate constraint notification_alert_policies_source_check",
            "compile-time closed typed owner-event source registry",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "migration 134 is missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "create index",
            "create trigger",
            "json_path",
            "jsonpath",
            "metric_value",
            "incident_state",
            "firing_count",
            "clock_timestamp",
            "pg_sleep",
            "next_evaluation",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 134 duplicated alert authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_alert_policy_certificate_expiry_source_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/135_notification_alert_policy_certificate_expiry_source.sql"
    ));

    #[test]
    fn migration_135_only_widens_the_closed_alert_source_registry() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "drop constraint notification_alert_policies_source_check",
            "add constraint notification_alert_policies_source_check",
            "edge.domain-claim-status.v1",
            "edge.gateway-certificate-renewal-status.v1",
            "workload.deployment-health.v1",
            "edge.gateway-certificate-expiry-status.v1",
            "not valid",
            "validate constraint notification_alert_policies_source_check",
            "compile-time closed typed owner-event source registry",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "migration 135 is missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "create index",
            "create trigger",
            "json_path",
            "jsonpath",
            "metric_value",
            "incident_state",
            "firing_count",
            "clock_timestamp",
            "pg_sleep",
            "next_evaluation",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 135 duplicated alert authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod identity_recipient_contact_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/136_identity_recipient_contacts.sql"
    ));

    #[test]
    fn migration_136_keeps_mailbox_pii_in_identity_and_proofs_ephemeral() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "create table recipient_contacts",
            "create table recipient_contact_verifications",
            "unique (principal_id, canonical_address)",
            "octet_length(canonical_address) = char_length(canonical_address)",
            "([.][a-z0-9",
            "expires_at >= issued_at + interval '1 minute'",
            "expires_at <= issued_at + interval '30 minutes'",
            "recipient_contact_verifications_pending_idx",
            "validate_recipient_contact_principal",
            "kind = 'human'",
            "disabled_at is null",
            "validate_recipient_contact_verification_insert",
            "organization_memberships",
            "state = 'pending'",
            "validate_recipient_contact_transition",
            "revoked recipient contacts are terminal",
            "reject_recipient_contact_delete",
            "validate_recipient_contact_verification_transition",
            "reject_recipient_contact_verification_delete",
            "proof and signature material are never persisted",
        ] {
            assert!(
                lower.contains(expected),
                "migration 136 is missing {expected}"
            );
        }
        for forbidden in [
            "create table notification",
            "create table delivery",
            "provider_message",
            "secret_material",
            "proof text",
            "signature text",
            "mailbox text",
            "\\.",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 136 contains forbidden recipient-contact schema material {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod identity_recipient_contact_verification_delivery_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/137_identity_recipient_contact_verification_delivery.sql"
    ));

    #[test]
    fn migration_137_enforces_one_shot_redacted_smtp_evidence() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "create table recipient_contact_verification_deliveries",
            "verification_id uuid primary key references recipient_contact_verifications(id)",
            "'reserved', 'dispatching', 'delivered', 'rejected', 'indeterminate', 'obsolete'",
            "fence_token <> '00000000-0000-0000-0000-000000000000'",
            "lease_expires_at > reserved_at",
            "dispatch_started_at < lease_expires_at",
            "delivery must begin before dispatch",
            "reservation renewal is invalid",
            "dispatch fence changed",
            "terminal state is immutable",
            "reject_recipient_contact_verification_delivery_delete",
            "mailbox, proof, message bytes, credentials, and provider response text are forbidden",
        ] {
            assert!(
                lower.contains(expected),
                "migration 137 is missing {expected}"
            );
        }
        for forbidden in [
            "canonical_address",
            "proof text",
            "message_body",
            "smtp_password",
            "provider_response",
            "retry_count",
            "create table notification",
            "create table connector",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 137 persists forbidden material through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_outbound_smtp_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/138_notification_outbound_smtp.sql"
    ));

    #[test]
    fn migration_138_adds_exact_verified_contact_smtp_attempt_authority() {
        let lower = MIGRATION.to_ascii_lowercase();
        let canonical = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            "cloud.notification.outbound-subscription.v4",
            "a3s.cloud.notification-delivery.v3",
            "channel in ('signed_webhook', 'slack_compatible', 'smtp')",
            "notification_outbound_subscriptions_target_authority_check",
            "notification_outbound_deliveries_target_authority_check",
            "references recipient_contacts (principal_id, id)",
            "create table notification_outbound_smtp_attempts",
            "state text not null check (state in ('reserved', 'dispatching', 'terminal'))",
            "outcome text check ( outcome in ('accepted', 'rejected', 'retryable', 'indeterminate', 'obsolete') )",
            "lease_expires_at <= reserved_at + interval '5 minutes'",
            "outcome_deadline_at <= dispatch_started_at + interval '120 seconds'",
            "smtp notification attempt requires exact prior retryable evidence",
            "smtp notification reservation takeover is not fenced",
            "terminal smtp notification attempts are immutable",
            "outbound smtp terminal receipt does not match its exact notifications attempt",
            "attempt_state is distinct from 'terminal' or evidence_outcome is distinct from 'indeterminate'",
            "not a queue, scheduler, connector attempt, mailbox store, credential store, or provider-response store",
        ] {
            assert!(
                canonical.contains(expected),
                "migration 138 is missing {expected}"
            );
        }
        for forbidden in [
            "add column canonical_address",
            "add column address_digest",
            "add column contact_hint",
            "add column credential",
            "add column provider_response",
            "create table notification_queue",
            "create table notification_retries",
            "retry_count",
            "next_attempt_at",
        ] {
            assert!(
                !canonical.contains(forbidden),
                "migration 138 persists forbidden SMTP material through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod fleet_node_availability_fact_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/139_fleet_node_availability_facts.sql"
    ));

    #[test]
    fn migration_139_adds_a_bounded_strict_owner_fact_cursor() {
        let lower = MIGRATION.to_ascii_lowercase();
        let canonical = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            "create table fleet_node_availability_fact_heads",
            "state text not null check (state in ('observed', 'unavailable', 'resolved'))",
            "'fleet.node.unavailable'",
            "'fleet.node.availability-resolved'",
            "resolution_reason in ('heartbeat_restored', 'node_revoked')",
            "firing_timeout_deadline_at > firing_last_observed_at",
            "detected_at > firing_timeout_deadline_at",
            "where state in ('observed', 'resolved')",
            "fleet node availability fact heads cannot be deleted",
            "fleet node availability resolution does not advance its firing",
            "timeout-policy drift alone cannot create an availability fact",
            "not a generic health, incident, metric, queue, scheduler, timer, log, inventory, command, credential, provider-response, or notifications store",
        ] {
            assert!(canonical.contains(expected), "migration 139 is missing {expected}");
        }
        for forbidden in [
            "capabilities json",
            "inventory json",
            "command json",
            "metric json",
            "provider_response",
            "credential text",
            "diagnostic text",
            "create table notification",
            "create table incident",
            "create table queue",
        ] {
            assert!(
                !canonical.contains(forbidden),
                "migration 139 persists forbidden material through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod notification_alert_policy_node_target_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/140_notification_alert_policy_node_target.sql"
    ));

    #[test]
    fn migration_140_enforces_one_closed_environment_or_node_target() {
        let lower = MIGRATION.to_ascii_lowercase();
        let canonical = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            "alter column project_id drop not null",
            "alter column environment_id drop not null",
            "add column node_id uuid",
            "references nodes (organization_id, id)",
            "definition_schema = 'cloud.notification.alert-policy.v1'",
            "definition_schema = 'cloud.notification.alert-policy.v2'",
            "source = 'fleet.node-availability-status.v1'",
            "and project_id is null and environment_id is null and node_id is not null",
            "notification_alert_policies_active_environment_source_scope_idx",
            "notification_alert_policies_active_node_source_scope_idx",
            "notification_alert_policies_environment_source_scope_idx",
            "notification_alert_policies_node_source_scope_idx",
            "or new.node_id is distinct from old.node_id",
        ] {
            assert!(
                canonical.contains(expected),
                "migration 140 is missing {expected}"
            );
        }
        for forbidden in [
            "create table notification_incident",
            "create table node_health",
            "create table notification_queue",
            "threshold",
            "retry_count",
            "next_attempt_at",
        ] {
            assert!(
                !canonical.contains(forbidden),
                "migration 140 adds forbidden alert authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod security_gateway_route_policy_timeline_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/141_security_gateway_route_policy_timeline.sql"
    ));

    #[test]
    fn migration_141_adds_only_bounded_owner_fact_and_audit_query_indexes() {
        let canonical = MIGRATION
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for expected in [
            "create index outbox_events_security_gateway_route_policy_timeline_idx",
            "on outbox_events ( organization_id, aggregate_id, occurred_at desc, event_id desc )",
            "create index audit_records_security_gateway_route_policy_correlation_idx",
            "on audit_records ( organization_id, aggregate_id, action, occurred_at, request_id, audit_id )",
            "'edge.mcp-route-policy.created'",
            "'edge.mcp-route-policy.revised'",
            "audit details remain private",
        ] {
            assert!(
                canonical.contains(expected),
                "migration 141 is missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "alter table",
            "details json",
            "incident",
            "denial",
            "telemetry",
            "queue",
            "scheduler",
        ] {
            assert!(
                !canonical.contains(forbidden),
                "migration 141 adds forbidden security authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod audit_attribution_snapshot_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/142_audit_attribution_snapshots.sql"
    ));

    #[test]
    fn migration_142_extends_only_shared_audit_records_with_closed_immutable_attribution() {
        let canonical = MIGRATION
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for expected in [
            "alter table audit_records",
            "add column project_id uuid",
            "add column environment_id uuid",
            "add column attribution_profile_id uuid",
            "add column attribution_status text",
            "set attribution_status = 'legacy_unknown'",
            "attribution_status = 'not_applicable'",
            "attribution_status = 'profile_missing'",
            "attribution_status = 'profile_bound'",
            "references projects (organization_id, id)",
            "references environments (organization_id, project_id, id)",
            "references project_attribution_profiles (organization_id, project_id, id)",
            "audit_records_reject_new_legacy_attribution",
            "audit_records_attribution_immutable",
            "audit_records_project_attribution_query_idx",
            "audit_records_environment_attribution_query_idx",
            "audit_records_profile_attribution_query_idx",
            "audit_records_attribution_status_query_idx",
            "private details are never an attribution source",
        ] {
            assert!(
                canonical.contains(expected),
                "migration 142 is missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "details::",
            "details ->",
            "create table usage",
            "create table invoice",
            "create table price",
            "create table balance",
            "create table settlement",
            "create table entitlement",
            "create table queue",
            "create table export",
            "signing_key",
            "scheduler",
        ] {
            assert!(
                !canonical.contains(forbidden),
                "migration 142 adds forbidden authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod audit_retention_authority_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/144_audit_retention_authority.sql"
    ));

    #[test]
    fn migration_144_establishes_one_monotonic_audit_retention_authority() {
        let canonical = MIGRATION
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for expected in [
            "create table audit_retention_states",
            "organization_id uuid primary key references organizations(id) on delete cascade",
            "records_available_from timestamptz",
            "records_deleted_before timestamptz",
            "applied_policy_digest text",
            "total_deleted_records bigint not null default 0",
            "next_scan_at timestamptz not null default '1970-01-01 00:00:00+00'",
            "insert into audit_retention_states (organization_id) select id from organizations",
            "organizations_create_audit_retention_state",
            "audit_retention_states_monotonic",
            "new.version <> old.version + 1",
            "new.records_available_from < old.records_available_from",
            "new.records_deleted_before < old.records_deleted_before",
            "new.next_scan_at < old.next_scan_at",
            "new.last_swept_at < old.last_swept_at",
            "audit_records_enforce_retention_boundary",
            "from audit_retention_states state",
            "for share",
            "new.occurred_at < retained_from",
            "using errcode = '23514'",
            "audit_retention_states_next_scan_idx",
        ] {
            assert!(
                canonical.contains(expected),
                "migration 144 is missing {expected}"
            );
        }
        for forbidden in [
            "details::",
            "details ->",
            "create table export",
            "create table siem",
            "create table legal_hold",
            "signing_key",
            "pg_cron",
            "scheduler",
        ] {
            assert!(
                !canonical.contains(forbidden),
                "migration 144 duplicated audit authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod workload_writer_fence_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/131_workload_writer_fence_receipts.sql"
    ));

    #[test]
    fn migration_131_keeps_runtime_fences_immutable_and_operation_bound() {
        let lower = MIGRATION.to_ascii_lowercase();
        let canonical = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            "create table workload_writer_fence_receipts",
            "unique (node_id, id, aggregate_id, generation, command_kind, payload_digest)",
            "primary key (organization_id, workload_id, writer_epoch)",
            "command_kind = 'runtime_remove'",
            "foreign key ( node_id, command_id, replica_id, writer_epoch, command_kind, command_payload_digest ) references node_commands ( node_id, id, aggregate_id, generation, command_kind, payload_digest )",
            "foreign key (organization_id, continuation_operation_id)",
            "references operation_requests (organization_id, operation_id)",
            "cloud.workload.writer-fence-receipt.v1",
            "before update or delete",
            "workload writer-fence receipts are immutable",
            "owner-supplied continuation atomically enqueued",
        ] {
            assert!(
                canonical.contains(&expected.to_ascii_lowercase()),
                "migration 131 is missing {expected}"
            );
        }
        for forbidden in [
            "secret_value",
            "access_key_id",
            "secret_access_key",
            "create table object_namespace",
            "create table recovery",
            "retry_count",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 131 duplicated external authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod agent_code_command_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/132_agent_code_command_persistence.sql"
    ));

    #[test]
    fn migration_132_extends_the_existing_fleet_command_authority() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "drop constraint node_commands_command_kind_check",
            "add constraint node_commands_command_kind_check",
            "'runtime_apply'",
            "'box_build_start'",
            "'gateway_snapshot_install'",
            "'plugin_host_plan_enablement'",
            "'resource_claim_prepare'",
            "'code_agent_command'",
        ] {
            assert!(
                lower.contains(expected),
                "migration 132 is missing {expected}"
            );
        }
        for forbidden in ["create table", "agent_commands", "code_agent_commands"] {
            assert!(
                !lower.contains(forbidden),
                "migration 132 duplicated Fleet command authority through {forbidden}"
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
mod workflow_connector_step_projection_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/123_workflow_connector_step_projections.sql"
    ));

    #[test]
    fn migration_123_admits_only_the_wired_service_projection_and_failure_route_shape() {
        for expected in [
            "drop constraint workflow_step_projections_kind_check",
            "add constraint workflow_step_projections_kind_check check",
            "'execution'",
            "'service'",
            "drop constraint workflow_step_projections_selected_handle_routing_check",
            "add constraint workflow_step_projections_selected_handle_routing_check check",
            "selected_handle is null",
            "kind = 'branch'",
            "kind in ('execution', 'service')",
            "status = 'failed'",
            "exact ConnectorRevision binding",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in ["create table", "add column", "create queue", "retry"] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 123 added duplicate state or policy: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod workflow_agent_step_projection_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/161_workflow_agent_step_projections.sql"
    ));

    #[test]
    fn migration_161_admits_only_runtime_wired_projection_kinds_including_agent() {
        for expected in [
            "drop constraint workflow_step_projections_kind_check",
            "add constraint workflow_step_projections_kind_check check",
            "'agent'",
            "'execution'",
            "'service'",
            "'subworkflow'",
            "exact Assets-owned AgentRelease",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in ["create table", "add column", "selected_handle"] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 161 added unrelated persistence: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod workflow_agent_failure_step_projection_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/163_workflow_agent_failure_step_projections.sql"
    ));

    #[test]
    fn migration_163_admits_only_failed_agent_routing_evidence() {
        for expected in [
            "drop constraint workflow_step_projections_selected_handle_routing_check",
            "add constraint workflow_step_projections_selected_handle_routing_check check",
            "selected_handle is null",
            "kind = 'branch'",
            "kind in ('transform', 'execution', 'agent', 'service', 'output', 'subworkflow')",
            "status = 'failed'",
            "descriptor-bound Transform, Execution, Agent, Connector",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in ["create table", "add column", "create queue", "retry"] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 163 added duplicate state or policy: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod agent_provider_selection_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/164_agent_provider_selection.sql"
    ));

    #[test]
    fn migration_164_freezes_a_profile_before_runtime_dispatch() {
        for expected in [
            "where provider_kind is null",
            "alter column provider_kind set not null",
            "agent_executions_provider_binding_complete",
            "provider_node_id is null",
            "provider_node_id is not null",
            "canonical ACL and digests remain recovery authority",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in ["create table", "create queue", "jsonb", "provider_config"] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 164 added another authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod agent_harness_invocation_profile_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/165_agent_harness_invocation_profiles.sql"
    ));

    #[test]
    fn migration_165_adds_only_the_named_immutable_execution_binding() {
        for expected in [
            "add column invocation_profile jsonb",
            "add column invocation_profile_digest text",
            "agent_executions_invocation_profile_complete",
            "a3s.cloud.harness-invocation-profile.v1",
            "agent_executions_invocation_profile_immutable",
            "Agent Harness invocation profile must be bound before dispatch",
            "Agent Harness invocation profile is immutable",
            "never Secret material",
            "legacy unbound executions fail closed at redispatch",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in [
            "create table",
            "create queue",
            "secret_material",
            "provider_config",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 165 added another authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod agent_tool_event_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/166_agent_tool_events.sql"
    ));

    #[test]
    fn migration_166_extends_only_the_existing_semantic_sequence() {
        for expected in [
            "alter table agent_execution_events",
            "'tool_request'",
            "'tool_result'",
            "never Tool payload or Secret material",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in [
            "create table",
            "create queue",
            "tool_payload jsonb",
            "secret_material",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 166 added another authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod agent_approval_checkpoint_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/167_agent_approval_checkpoints.sql"
    ));

    #[test]
    fn migration_167_adds_one_exact_durable_approval_authority() {
        for expected in [
            "'awaiting_approval'",
            "'approval_resolved'",
            "create table agent_approval_checkpoints",
            "provider_run_identity_digest",
            "invocation_profile_digest",
            "agent_approval_checkpoints_one_active_per_execution_idx",
            "agent_approval_checkpoints_decision_idx",
            "references node_commands(id)",
            "interval '1 day'",
            "never Tool payload or Secret material",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in [
            "tool_payload jsonb",
            "request_payload",
            "secret_material",
            "create queue",
            "provider_config",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 167 duplicated payload, Secret, or queue authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod agent_execution_checkpoint_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/168_agent_execution_checkpoints.sql"
    ));

    #[test]
    fn migration_168_adds_only_checkpoint_descriptors_and_immutable_fork_lineage() {
        for expected in [
            "create table agent_execution_checkpoints",
            "object_namespace text not null check (object_namespace = 'agent-checkpoints')",
            "object_digest",
            "provider_run_identity_digest",
            "parent_execution_id",
            "parent_checkpoint_id",
            "parent_checkpoint_digest",
            "parent_checkpoint_digest is not null",
            "fork_depth is not null",
            "agent_executions_parent_checkpoint_fk",
            "shared immutable-object authority",
            "never mutates its parent trajectory",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in [
            "checkpoint_payload",
            "checkpoint_content",
            "secret_material",
            "create queue",
            "create table agent_execution_heads",
            "provider_config",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 168 duplicated content or lifecycle authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod agent_checkpoint_object_lease_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/169_agent_checkpoint_object_leases.sql"
    ));

    #[test]
    fn migration_169_adds_only_a_bounded_checkpoint_object_fence() {
        for expected in [
            "create table agent_execution_checkpoint_object_leases",
            "purpose in ('capture', 'inventory', 'cleanup')",
            "lease_expires_at > reserved_at",
            "agent_execution_checkpoint_object_leases_expiration_idx",
            "deliberately has no tenant foreign key",
            "stores no checkpoint payload",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in [
            "checkpoint_payload",
            "checkpoint_content",
            "secret_material",
            "create queue",
            "references organizations",
            "references agent_executions",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 169 duplicated payload, queue, or lifecycle authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod user_file_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/170_user_files.sql"
    ));

    #[test]
    fn migration_170_keeps_one_acl_lifecycle_quota_and_outbox_authority() {
        for expected in [
            "create table user_file_organization_quotas",
            "create table user_files",
            "contract_schema = 'cloud.user-file.v1'",
            "canonical_acl text not null",
            "primary key (organization_id, id)",
            "limit_bytes between 1 and 9007199254740991",
            "allocated_bytes >= 0 and allocated_bytes <= limit_bytes",
            "revision between 0 and 9007199254740991",
            "user_files_upload_expiration_idx",
            "user_files_cleanup_due_idx",
            "bytes remain in the shared immutable-object authority",
            "shared Outbox rather than a Files-local queue",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in [
            "create queue",
            "file_bytes",
            "object_payload",
            "provider_config",
            "bucket_name",
            "access_key",
            "json_config",
            "yaml_config",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 170 duplicated configuration, bytes, queue, or provider authority: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod workflow_application_answer_step_projection_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/143_workflow_application_answer_step_projections.sql"
    ));

    #[test]
    fn migration_143_admits_only_failed_output_routing_evidence() {
        for expected in [
            "drop constraint workflow_step_projections_selected_handle_routing_check",
            "add constraint workflow_step_projections_selected_handle_routing_check check",
            "selected_handle is null",
            "kind = 'branch'",
            "kind in ('execution', 'service', 'output')",
            "status = 'failed'",
            "Application Answer route",
            "immutable WorkflowRun plan",
        ] {
            assert!(MIGRATION.contains(expected), "missing {expected}");
        }
        for forbidden in ["create table", "add column", "create queue", "retry"] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 143 added duplicate state or policy: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod application_release_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/124_application_releases.sql"
    ));

    #[test]
    fn migration_124_adds_one_immutable_application_release_authority() {
        for expected in [
            "create table applications",
            "create table application_releases",
            "applications_current_release_fk",
            "deferrable initially deferred",
            "references workflow_revisions",
            "validate_application_release_lineage",
            "validate_application_release_workflow_binding",
            "reject_application_release_mutation",
            "validate_application_update",
            "validate_application_head",
            "cloud.application.release.v1",
            "application releases are immutable",
        ] {
            assert!(
                MIGRATION
                    .to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase()),
                "missing {expected}"
            );
        }
        for forbidden in [
            "session_state",
            "flow_history",
            "provider_endpoint",
            "secret_material",
            "gateway_route",
            "create queue",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 124 duplicated non-Applications authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod application_session_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/125_application_sessions.sql"
    ));

    #[test]
    fn migration_125_adds_one_release_pinned_session_authority() {
        for expected in [
            "create table application_end_users",
            "create table application_sessions",
            "create table application_invocations",
            "create table application_messages",
            "create table application_conversation_variable_revisions",
            "create table application_workflow_effect_claims",
            "references workflow_runs",
            "application_sessions_variable_head_fk",
            "deferrable initially deferred",
            "validate_application_session_update",
            "validate_application_invocation_update",
            "validate_application_effect_claim",
            "Application session semantic children are immutable",
            "not a Flow event log",
        ] {
            assert!(
                MIGRATION
                    .to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase()),
                "missing {expected}"
            );
        }
        for forbidden in [
            "flow_history",
            "provider_output",
            "provider_state",
            "create queue",
            "retry_count",
            "secret_material",
            "membership_role",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 125 duplicated another authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod application_invocation_workflow_authority_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/126_application_invocation_workflow_authority.sql"
    ));

    #[test]
    fn migration_126_retains_one_immutable_restart_authority() {
        for expected in [
            "create table application_invocation_workflow_authorities",
            "references application_invocations",
            "references ontology_revisions",
            "references environments",
            "validate_application_invocation_workflow_authority",
            "deferrable initially deferred",
            "reject_application_session_child_mutation",
            "immutable external revision and caller authority",
        ] {
            assert!(
                MIGRATION
                    .to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase()),
                "missing {expected}"
            );
        }
        for forbidden in [
            "flow_history",
            "provider_output",
            "provider_state",
            "create queue",
            "retry_count",
            "secret_material",
            "membership_role",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration 126 duplicated another authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod application_invocation_timeout_policy_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/127_application_invocation_timeout_policy.sql"
    ));

    #[test]
    fn migration_127_matches_the_ordinary_workflow_run_timeout_bound() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "alter table application_invocation_workflow_authorities",
            "application_invocation_workflow_authorities_timeout_policy",
            "timeout_seconds <= 2592000",
            "ordinary WorkflowRun 30-day admission bound",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "missing {expected}"
            );
        }
        for forbidden in [
            "create table",
            "update application_invocation_workflow_authorities",
            "delete from application_invocation_workflow_authorities",
            "flow_history",
            "provider_state",
            "secret_material",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 127 duplicated or rewrote authority through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod application_invocation_timeout_policy_owner_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/171_application_invocation_timeout_policy_owner.sql"
    ));

    #[test]
    fn migration_171_removes_the_copied_workflow_timeout_policy() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "alter table application_invocation_workflow_authorities",
            "drop constraint application_invocation_workflow_authorities_timeout_policy",
            "workflow alone owns its default and maximum",
        ] {
            assert!(
                lower.contains(&expected.to_ascii_lowercase()),
                "missing {expected}"
            );
        }
        for forbidden in [
            "add constraint",
            "2592000",
            "30-day",
            "create table",
            "update application_invocation_workflow_authorities",
            "delete from application_invocation_workflow_authorities",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 171 retained or replaced the copied timeout policy through {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod human_task_submission_owner_migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/173_human_task_submission_owner.sql"
    ));

    #[test]
    fn migration_173_corrects_ownership_without_rewriting_historical_evidence() {
        let lower = MIGRATION.to_ascii_lowercase();
        for expected in [
            "comment on table form_submissions",
            "workflow-owned immutable humantasksubmission evidence",
            "historical table name preserves replay compatibility",
            "forms owns definitions, releases, and semantic evaluation",
        ] {
            assert!(lower.contains(expected), "missing {expected}");
        }
        for forbidden in [
            "alter table",
            "create table",
            "update form_submissions",
            "delete from form_submissions",
            "insert into form_submissions",
        ] {
            assert!(
                !lower.contains(forbidden),
                "migration 173 rewrote HumanTask evidence through {forbidden}"
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
