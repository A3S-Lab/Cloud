//! Fleet-owned same-transaction participant for node organization locks.
//!
//! Edge staging paths that must prove a node belongs to one organization
//! inside their own Postgres transaction call this helper instead of mapping
//! the Fleet `nodes` table.

use super::schema::Nodes;
use crate::infrastructure::{PostgresPersistenceError, fetch_optional};
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId, RepositoryError};
use a3s_orm::{PostgresTransaction, select_from};
use uuid::Uuid;

/// Lock one Fleet node row and require it to belong to `organization_id`.
///
/// Missing nodes and organization mismatches both fail closed as `NotFound`.
pub(crate) async fn lock_node_organization_for_update(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    node_id: NodeId,
) -> Result<(), PostgresPersistenceError> {
    let locked_organization_id = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<Nodes>()
            .select(Nodes::organization_id())
            .filter(Nodes::id().eq(node_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    if locked_organization_id != organization_id.as_uuid() {
        return Err(RepositoryError::NotFound.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::{PostgresDialect, Query};

    #[test]
    fn node_organization_lock_query_targets_nodes_with_for_update() {
        let query = select_from::<Nodes>()
            .select(Nodes::organization_id())
            .filter(Nodes::id().eq(Uuid::nil()))
            .for_update()
            .compile(&PostgresDialect)
            .expect("node organization lock query");
        assert!(query.sql.contains(" from \"nodes\" "));
        assert!(query.sql.ends_with(" for update"));
        assert_eq!(query.parameters.len(), 1);
    }
}
