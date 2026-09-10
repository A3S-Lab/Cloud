use crate::modules::fleet::domain::repositories::INodePoolRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::{IWorkloadsNodePoolAccess, WorkloadsNodePoolScope};
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Fleet node-pool authority.
#[derive(Clone)]
pub struct FleetWorkloadsNodePoolAccessAdapter {
    node_pools: Arc<dyn INodePoolRepository>,
}

impl FleetWorkloadsNodePoolAccessAdapter {
    pub fn new(node_pools: Arc<dyn INodePoolRepository>) -> Self {
        Self { node_pools }
    }
}

#[async_trait]
impl IWorkloadsNodePoolAccess for FleetWorkloadsNodePoolAccessAdapter {
    async fn node_pool_exists(
        &self,
        scope: WorkloadsNodePoolScope,
    ) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        match self
            .node_pools
            .find(scope.organization_id(), scope.node_pool_id())
            .await
        {
            Ok(pool)
                if pool.organization_id == scope.organization_id()
                    && pool.id == scope.node_pool_id() =>
            {
                Ok(true)
            }
            Ok(_) => Err(RepositoryError::Storage(
                "Fleet returned inconsistent Workloads node-pool evidence".into(),
            )),
            Err(RepositoryError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::fleet::domain::entities::NodePool;
    use crate::modules::fleet::domain::repositories::NodePoolWrite;
    use crate::modules::shared_kernel::domain::{
        IdempotencyRequest, IdempotentWrite, NodeId, NodePoolId, OrganizationId, ResourceName,
    };
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubNodePoolRepository {
        pool: Option<NodePool>,
        find_calls: AtomicUsize,
    }

    #[async_trait]
    impl INodePoolRepository for StubNodePoolRepository {
        async fn replay(
            &self,
            _idempotency: &IdempotencyRequest,
        ) -> Result<Option<NodePool>, RepositoryError> {
            unreachable!("node-pool access adapter never replays writes")
        }

        async fn save(
            &self,
            _write: NodePoolWrite,
        ) -> Result<IdempotentWrite<NodePool>, RepositoryError> {
            unreachable!("node-pool access adapter never saves pools")
        }

        async fn find(
            &self,
            organization_id: OrganizationId,
            pool_id: NodePoolId,
        ) -> Result<NodePool, RepositoryError> {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            match &self.pool {
                Some(pool) if pool.organization_id == organization_id && pool.id == pool_id => {
                    Ok(pool.clone())
                }
                Some(_) | None => Err(RepositoryError::NotFound),
            }
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
        ) -> Result<Vec<NodePool>, RepositoryError> {
            unreachable!("node-pool access adapter never lists pools")
        }
    }

    #[tokio::test]
    async fn adapter_projects_only_exact_existing_node_pool_evidence() {
        let organization_id = OrganizationId::new();
        let node_pool_id = NodePoolId::new();
        let pool = NodePool::create(
            node_pool_id,
            organization_id,
            ResourceName::parse("workers").expect("node pool name"),
            vec![NodeId::new()],
            Utc::now(),
        )
        .expect("node pool");
        let present = FleetWorkloadsNodePoolAccessAdapter::new(Arc::new(StubNodePoolRepository {
            pool: Some(pool),
            find_calls: AtomicUsize::new(0),
        }));
        let scope = WorkloadsNodePoolScope::new(organization_id, node_pool_id).unwrap();
        assert!(present.node_pool_exists(scope).await.unwrap());

        let missing = FleetWorkloadsNodePoolAccessAdapter::new(Arc::new(StubNodePoolRepository {
            pool: None,
            find_calls: AtomicUsize::new(0),
        }));
        assert!(!missing.node_pool_exists(scope).await.unwrap());
    }
}
