use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, RepositoryError, WorkflowDefinitionId,
};
use crate::modules::workflow::domain::{
    WorkflowAuthoringAppend, WorkflowAuthoringEntry, WorkflowAuthoringJournal,
    WorkflowAuthoringOperation, WorkflowAuthoringPage, WorkflowAuthoringSnapshot,
};
use async_trait::async_trait;
use uuid::Uuid;

/// Request metadata required to make a hosted authoring mutation auditable.
///
/// The application layer obtains this from authenticated Cloud request context;
/// Flow never sees it and it is never inferred from an opaque DSL payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowAuthoringWriteContext {
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
}

impl WorkflowAuthoringWriteContext {
    pub const fn new(actor_principal_id: PrincipalId, request_id: Uuid) -> Self {
        Self {
            actor_principal_id,
            request_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.actor_principal_id.as_uuid().is_nil() || self.request_id.is_nil() {
            return Err("workflow authoring write context is invalid".into());
        }
        Ok(())
    }
}

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

    /// Creates a journal and, in durable production adapters, records its
    /// Outbox and audit facts in the same transaction. The compatibility
    /// default is retained for lightweight/custom adapters that predate the
    /// hosted audit contract; the Cloud PostgreSQL adapter overrides it.
    async fn create_with_context(
        &self,
        write: CreateWorkflowAuthoringJournal,
        _context: WorkflowAuthoringWriteContext,
    ) -> Result<WorkflowAuthoringJournal, RepositoryError> {
        self.create(write).await
    }

    async fn append(
        &self,
        write: AppendWorkflowAuthoringOperation,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError>;

    /// Appends a journal entry and, in durable production adapters, records
    /// the corresponding Outbox and audit facts atomically. See
    /// [`Self::create_with_context`] for the compatibility rationale.
    async fn append_with_context(
        &self,
        write: AppendWorkflowAuthoringOperation,
        _context: WorkflowAuthoringWriteContext,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError> {
        self.append(write).await
    }

    /// Reads only the materialized head needed for an append preflight.
    ///
    /// Implementations should answer this from the journal head row rather
    /// than rehydrating every historical entry. The default keeps custom
    /// adapters source-compatible while they migrate to the bounded query.
    async fn current_snapshot(
        &self,
        key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringSnapshot>, RepositoryError> {
        Ok(self
            .find(key)
            .await?
            .map(|journal| journal.current_snapshot().clone()))
    }

    /// Finds one immutable operation entry by its idempotency key.
    ///
    /// The returned entry is already validated by the repository adapter. A
    /// missing entry is distinct from an idempotency conflict: callers compare
    /// the stored digest with the incoming operation before deciding whether
    /// to replay or reject it. The default implementation is intentionally a
    /// compatibility fallback over [`Self::find`].
    async fn find_operation(
        &self,
        key: WorkflowAuthoringJournalKey,
        operation_id: &str,
    ) -> Result<Option<WorkflowAuthoringEntry>, RepositoryError> {
        Ok(self.find(key).await?.and_then(|journal| {
            journal
                .entries()
                .iter()
                .find(|entry| entry.operation_id() == operation_id)
                .cloned()
        }))
    }

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
