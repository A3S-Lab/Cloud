use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectId, RepositoryError, WorkflowDefinitionId,
};
use crate::modules::workflow::domain::{
    WorkflowAuthoringAppend, WorkflowAuthoringJournal, WorkflowAuthoringOperation,
    WorkflowAuthoringPage, WorkflowAuthoringSnapshot,
};
use async_trait::async_trait;

/// Tenant-scoped identity of a hosted workflow authoring journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkflowAuthoringJournalKey {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_definition_id: WorkflowDefinitionId,
}

impl WorkflowAuthoringJournalKey {
    pub const fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            workflow_definition_id,
        }
    }

    /// Validates the tenant and aggregate identity before it crosses an
    /// application or persistence boundary.
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_definition_id.as_uuid().is_nil()
        {
            return Err("workflow authoring journal identity is invalid".into());
        }
        Ok(())
    }
}

/// Input for creating the first materialized snapshot of a hosted journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorkflowAuthoringJournal {
    pub key: WorkflowAuthoringJournalKey,
    pub initial_snapshot: WorkflowAuthoringSnapshot,
}

/// Input for appending one Flow-validated operation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendWorkflowAuthoringOperation {
    pub key: WorkflowAuthoringJournalKey,
    pub operation: WorkflowAuthoringOperation,
    pub result_snapshot: WorkflowAuthoringSnapshot,
}

/// Repository port for hosted authoring state.
///
/// Implementations own tenant-scoped durable storage and transaction isolation;
/// they must not decode the operation or snapshot bytes. Authorization belongs in
/// the application layer before this port is called.
#[async_trait]
pub trait IWorkflowAuthoringRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateWorkflowAuthoringJournal,
    ) -> Result<WorkflowAuthoringJournal, RepositoryError>;

    async fn append(
        &self,
        write: AppendWorkflowAuthoringOperation,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError>;

    async fn find(
        &self,
        key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringJournal>, RepositoryError>;

    async fn page(
        &self,
        key: WorkflowAuthoringJournalKey,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Option<WorkflowAuthoringPage>, RepositoryError>;
}
