use crate::modules::operations::domain::entities::{
    OperationProjection, OperationRecord, OperationRequest,
};
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OperationId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryOperationRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    requests: BTreeMap<OperationId, OperationRequest>,
    projections: BTreeMap<OperationId, OperationProjection>,
}

impl InMemoryOperationRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IOperationRepository for InMemoryOperationRepository {
    async fn enqueue(
        &self,
        request: OperationRequest,
    ) -> Result<IdempotentWrite<OperationRequest>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(existing) = state.requests.get(&request.id) {
            if !existing.has_same_definition(&request) {
                return Err(RepositoryError::Conflict(
                    "operation ID was reused with a different request".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        state.requests.insert(request.id, request.clone());
        Ok(IdempotentWrite {
            value: request,
            replayed: false,
        })
    }

    async fn pending_starts(&self, limit: usize) -> Result<Vec<OperationRequest>, RepositoryError> {
        let state = self.state.read().await;
        let mut requests = state
            .requests
            .values()
            .filter(|request| {
                state
                    .projections
                    .get(&request.id)
                    .is_none_or(|projection| !projection.status.is_terminal())
            })
            .cloned()
            .collect::<Vec<_>>();
        requests.sort_by_key(|request| (request.requested_at, request.id));
        requests.truncate(limit);
        Ok(requests)
    }

    async fn find_request(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationRequest>, RepositoryError> {
        Ok(self.state.read().await.requests.get(&operation_id).cloned())
    }

    async fn upsert_projection(
        &self,
        projection: OperationProjection,
    ) -> Result<(), RepositoryError> {
        let mut state = self.state.write().await;
        if !state.requests.contains_key(&projection.operation_id) {
            return Err(RepositoryError::NotFound);
        }
        if let Some(existing) = state.projections.get(&projection.operation_id) {
            if existing.last_sequence > projection.last_sequence {
                return Ok(());
            }
            if existing.last_sequence == projection.last_sequence
                && (existing.status != projection.status
                    || existing.output != projection.output
                    || existing.error != projection.error)
            {
                return Err(RepositoryError::Storage(
                    "operation projection changed without advancing its sequence".into(),
                ));
            }
        }
        state
            .projections
            .insert(projection.operation_id, projection);
        Ok(())
    }

    async fn find_projection(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationProjection>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .projections
            .get(&operation_id)
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        limit: usize,
    ) -> Result<Vec<OperationRecord>, RepositoryError> {
        let state = self.state.read().await;
        let mut records = state
            .requests
            .values()
            .filter(|request| request.organization_id == organization_id)
            .map(|request| OperationRecord {
                request: request.clone(),
                projection: state.projections.get(&request.id).cloned(),
            })
            .collect::<Vec<_>>();
        records.sort_by_key(|record| {
            (
                std::cmp::Reverse(record.request.requested_at),
                record.request.id,
            )
        });
        records.truncate(limit.max(1));
        Ok(records)
    }
}
