use crate::modules::workflow::domain::WorkflowNodeCatalog;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct WorkflowNodeCatalogResponse(WorkflowNodeCatalog);

impl From<WorkflowNodeCatalog> for WorkflowNodeCatalogResponse {
    fn from(value: WorkflowNodeCatalog) -> Self {
        Self(value)
    }
}
