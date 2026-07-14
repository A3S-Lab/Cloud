use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::services::IOperationEngine;
use crate::modules::shared_kernel::domain::{OperationId, RepositoryError};
use std::sync::Arc;

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
}

impl ReconcileOperationsHandler {
    pub fn new(
        repository: Arc<dyn IOperationRepository>,
        engine: Arc<dyn IOperationEngine>,
    ) -> Self {
        Self { repository, engine }
    }

    pub async fn execute(
        &self,
        limit: usize,
    ) -> Result<ReconcileOperationsReport, RepositoryError> {
        let requests = self.repository.pending_starts(limit.max(1)).await?;
        let mut report = ReconcileOperationsReport {
            inspected: requests.len(),
            ..ReconcileOperationsReport::default()
        };
        for request in requests {
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
                Ok(()) => report.projected += 1,
                Err(error) => report.failures.push(OperationReconcileFailure {
                    operation_id: request.id,
                    error: error.to_string(),
                }),
            }
        }
        Ok(report)
    }
}
