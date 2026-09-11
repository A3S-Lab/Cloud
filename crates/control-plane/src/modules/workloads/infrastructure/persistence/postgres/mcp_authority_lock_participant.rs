//! Workloads-owned same-transaction participant for MCP snapshot authority locks.
//!
//! Edge MCP Gateway snapshot staging proves each referenced Workload still holds
//! one exact running authority inside the caller transaction through this helper
//! instead of mapping the Workloads table.

use super::schema::Workloads;
use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, WorkloadId, WorkloadRevisionId,
};
use a3s_orm::{select_from, PostgresTransaction};
use uuid::Uuid;

/// Exact running Workload authority Edge observed while compiling an MCP snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpWorkloadAuthorityExpectation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub active_revision_id: WorkloadRevisionId,
    pub aggregate_version: u64,
}

type AuthorityRow = (Uuid, Uuid, Uuid, String, Option<Uuid>, u64);

/// Lock one Workload row and require it to still match the expected running authority.
pub(crate) async fn lock_running_workload_authority_for_update(
    transaction: &PostgresTransaction,
    workload_id: WorkloadId,
    expected: &McpWorkloadAuthorityExpectation,
) -> Result<(), PostgresPersistenceError> {
    let row = fetch_optional::<AuthorityRow, _>(
        transaction,
        select_from::<Workloads>()
            .select((
                Workloads::organization_id(),
                Workloads::project_id(),
                Workloads::environment_id(),
                Workloads::desired_state(),
                Workloads::active_revision_id(),
                Workloads::aggregate_version(),
            ))
            .filter(Workloads::id().eq(workload_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or_else(|| {
        RepositoryError::Conflict(
            "MCP snapshot Workload disappeared before Gateway staging".into(),
        )
    })?;
    if row
        != (
            expected.organization_id.as_uuid(),
            expected.project_id.as_uuid(),
            expected.environment_id.as_uuid(),
            "running".to_owned(),
            Some(expected.active_revision_id.as_uuid()),
            expected.aggregate_version,
        )
    {
        return Err(RepositoryError::Conflict(
            "MCP snapshot Workload authority changed before Gateway staging".into(),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::{PostgresDialect, Query};

    #[test]
    fn workload_authority_lock_query_targets_workloads_with_for_update() {
        let query = select_from::<Workloads>()
            .select((
                Workloads::organization_id(),
                Workloads::project_id(),
                Workloads::environment_id(),
                Workloads::desired_state(),
                Workloads::active_revision_id(),
                Workloads::aggregate_version(),
            ))
            .filter(Workloads::id().eq(Uuid::nil()))
            .for_update()
            .compile(&PostgresDialect)
            .expect("workload authority lock query");
        assert!(query.sql.contains(" from \"workloads\" "));
        assert!(query.sql.ends_with(" for update"));
        assert_eq!(query.parameters.len(), 1);
    }
}
