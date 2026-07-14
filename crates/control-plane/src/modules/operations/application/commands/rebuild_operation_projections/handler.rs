use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::services::{IOperationEngine, OperationEngineError};
use crate::modules::shared_kernel::domain::OperationId;
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildOperationProjectionsReport {
    pub inspected: usize,
    pub rebuilt: usize,
    pub orphaned: Vec<OperationId>,
}

#[derive(Debug, thiserror::Error)]
pub enum RebuildOperationProjectionsError {
    #[error(transparent)]
    Engine(#[from] OperationEngineError),
    #[error("operation projection repository failed: {0}")]
    Repository(String),
}

pub struct RebuildOperationProjectionsHandler {
    repository: Arc<dyn IOperationRepository>,
    engine: Arc<dyn IOperationEngine>,
}

impl RebuildOperationProjectionsHandler {
    pub fn new(
        repository: Arc<dyn IOperationRepository>,
        engine: Arc<dyn IOperationEngine>,
    ) -> Self {
        Self { repository, engine }
    }

    pub async fn execute(
        &self,
    ) -> Result<RebuildOperationProjectionsReport, RebuildOperationProjectionsError> {
        let projections = self.engine.projections().await?;
        let mut report = RebuildOperationProjectionsReport {
            inspected: projections.len(),
            ..RebuildOperationProjectionsReport::default()
        };
        for projection in projections {
            let operation_id = projection.operation_id;
            let request = self
                .repository
                .find_request(operation_id)
                .await
                .map_err(|error| RebuildOperationProjectionsError::Repository(error.to_string()))?;
            if request.is_none() {
                report.orphaned.push(operation_id);
                continue;
            }
            self.repository
                .upsert_projection(projection)
                .await
                .map_err(|error| RebuildOperationProjectionsError::Repository(error.to_string()))?;
            report.rebuilt += 1;
        }
        Ok(report)
    }
}
