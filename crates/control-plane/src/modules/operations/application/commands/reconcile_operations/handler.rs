use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::{
    IOperationRepository, OperationRefreshCursor,
};
use crate::modules::operations::domain::services::IOperationEngine;
use crate::modules::shared_kernel::domain::{OperationId, RepositoryError};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationReconcileFailure {
    pub operation_id: OperationId,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileOperationsReport {
    pub inspected: usize,
    pub projected: usize,
    pub failures: Vec<OperationReconcileFailure>,
}

pub struct ReconcileOperationsHandler {
    repository: Arc<dyn IOperationRepository>,
    engine: Arc<dyn IOperationEngine>,
    refresh_cursor: Mutex<Option<OperationRefreshCursor>>,
}

impl ReconcileOperationsHandler {
    pub fn new(
        repository: Arc<dyn IOperationRepository>,
        engine: Arc<dyn IOperationEngine>,
    ) -> Self {
        Self {
            repository,
            engine,
            refresh_cursor: Mutex::new(None),
        }
    }

    pub async fn execute(
        &self,
        limit: usize,
    ) -> Result<ReconcileOperationsReport, RepositoryError> {
        let limit = limit.max(1);
        let starts = self.repository.pending_starts(limit).await?;
        let refreshes = self.next_active_refreshes(limit).await?;
        let mut report = ReconcileOperationsReport {
            inspected: starts.len() + refreshes.len(),
            ..ReconcileOperationsReport::default()
        };
        for request in starts.into_iter().chain(refreshes) {
            let projection = match self.engine.ensure(&request).await {
                Ok(projection) => projection,
                Err(error) => {
                    report.failures.push(OperationReconcileFailure {
                        operation_id: request.id,
                        error: error.to_string(),
                    });
                    continue;
                }
            };
            match self.repository.upsert_projection(projection).await {
                Ok(true) => report.projected += 1,
                Ok(false) => {}
                Err(error) => report.failures.push(OperationReconcileFailure {
                    operation_id: request.id,
                    error: error.to_string(),
                }),
            }
        }
        Ok(report)
    }

    async fn next_active_refreshes(
        &self,
        limit: usize,
    ) -> Result<Vec<OperationRequest>, RepositoryError> {
        let mut cursor = self.refresh_cursor.lock().await;
        let mut requests = self.repository.active_refreshes(*cursor, limit).await?;
        if requests.is_empty() && cursor.is_some() {
            *cursor = None;
            requests = self.repository.active_refreshes(None, limit).await?;
        }
        *cursor = if requests.len() == limit {
            requests.last().map(OperationRefreshCursor::after)
        } else {
            None
        };
        Ok(requests)
    }
}
