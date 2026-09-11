//! Application orchestration for the hosted workflow authoring journal.
//!
//! Cloud owns authorization and durable journal access here. The Flow adapter
//! remains the only component allowed to interpret or apply the opaque DSL
//! operation bytes.

use super::resource_access::{self, WorkflowAccess};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::PrincipalId;
use crate::modules::workflow::domain::{
    AppendWorkflowAuthoringOperation, CreateWorkflowAuthoringJournal, IWorkflowAuthoringRepository,
    IWorkflowDefinitionRepository, WorkflowAuthoringAppend, WorkflowAuthoringError,
    WorkflowAuthoringJournal, WorkflowAuthoringJournalKey, WorkflowAuthoringOperation,
    WorkflowAuthoringPage, WorkflowAuthoringSnapshot, WorkflowAuthoringWriteContext,
    WORKFLOW_AUTHORING_MAX_PAGE_SIZE,
};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

const AUTHORING_NOT_FOUND: &str = "Workflow authoring journal not found";

/// Request to create the first hosted snapshot for a Workflow definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorkflowAuthoringJournalRequest {
    pub key: WorkflowAuthoringJournalKey,
    pub initial_snapshot: WorkflowAuthoringSnapshot,
    pub access: WorkflowAccess,
    /// Authenticated Cloud actor recorded in the shared audit trail.
    pub actor_principal_id: PrincipalId,
    /// Stable request/correlation identity for Outbox and audit records.
    pub request_id: Uuid,
}

impl CreateWorkflowAuthoringJournalRequest {
    fn validate_identity(&self) -> Result<(), String> {
        self.key.validate()?;
        WorkflowAuthoringWriteContext::new(self.actor_principal_id, self.request_id).validate()
    }

    fn write_context(&self) -> WorkflowAuthoringWriteContext {
        WorkflowAuthoringWriteContext::new(self.actor_principal_id, self.request_id)
    }
}

/// Request to apply one Flow operation to the current hosted snapshot.
///
/// The result snapshot is deliberately not supplied by the caller. The Flow
/// port computes it from the journal's current snapshot, so Cloud never
/// persists an unvalidated result assembled by a transport client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendWorkflowAuthoringRequest {
    pub key: WorkflowAuthoringJournalKey,
    pub operation: WorkflowAuthoringOperation,
    pub access: WorkflowAccess,
    /// Authenticated Cloud actor recorded in the shared audit trail.
    pub actor_principal_id: PrincipalId,
    /// Stable request/correlation identity for Outbox and audit records.
    pub request_id: Uuid,
}

impl AppendWorkflowAuthoringRequest {
    fn validate_identity(&self) -> Result<(), String> {
        self.key.validate()?;
        WorkflowAuthoringWriteContext::new(self.actor_principal_id, self.request_id).validate()
    }

    fn write_context(&self) -> WorkflowAuthoringWriteContext {
        WorkflowAuthoringWriteContext::new(self.actor_principal_id, self.request_id)
    }
}

/// Request to read a complete materialized authoring journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetWorkflowAuthoringJournalRequest {
    pub key: WorkflowAuthoringJournalKey,
    pub access: WorkflowAccess,
}

impl GetWorkflowAuthoringJournalRequest {
    fn validate_identity(&self) -> Result<(), String> {
        self.key.validate()
    }
}

/// Request to read an exclusive-cursor page of authoring operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageWorkflowAuthoringRequest {
    pub key: WorkflowAuthoringJournalKey,
    pub after_sequence: Option<u64>,
    pub limit: usize,
    pub access: WorkflowAccess,
}

impl PageWorkflowAuthoringRequest {
    fn validate_identity(&self) -> Result<(), String> {
        self.key.validate()
    }
}

/// Adapter boundary to the Flow authoring API.
///
/// Implementations belong in Cloud infrastructure and must delegate to
/// Flow's public authoring/parser surface. They must not be implemented by
/// decoding the operation bytes in this bounded context.
#[async_trait]
pub trait IWorkflowAuthoringFlowPort: Send + Sync {
    /// Validate and canonicalize a complete snapshot before it becomes the
    /// journal's initial materialized state.
    async fn validate_snapshot(
        &self,
        snapshot: &WorkflowAuthoringSnapshot,
    ) -> ApplicationResult<WorkflowAuthoringSnapshot>;

    /// Apply one operation to a known base and return Flow's validated result
    /// snapshot. The returned value must retain its canonical digest.
    async fn apply_operation(
        &self,
        base_snapshot: &WorkflowAuthoringSnapshot,
        operation: &WorkflowAuthoringOperation,
    ) -> ApplicationResult<WorkflowAuthoringSnapshot>;
}

/// Authorized application boundary for hosted authoring CRUD and cursors.
#[async_trait]
pub trait IWorkflowAuthoringApplicationPort: Send + Sync {
    /// Create a hosted authoring journal.
    async fn create_journal(
        &self,
        request: CreateWorkflowAuthoringJournalRequest,
    ) -> ApplicationResult<WorkflowAuthoringJournal>;

    /// Apply or idempotently replay one authoring operation.
    async fn append_operation(
        &self,
        request: AppendWorkflowAuthoringRequest,
    ) -> ApplicationResult<WorkflowAuthoringAppend>;

    /// Read an authorized materialized journal.
    async fn get_journal(
        &self,
        request: GetWorkflowAuthoringJournalRequest,
    ) -> ApplicationResult<WorkflowAuthoringJournal>;

    /// Read an authorized exclusive-cursor page.
    async fn page_operations(
        &self,
        request: PageWorkflowAuthoringRequest,
    ) -> ApplicationResult<WorkflowAuthoringPage>;
}

/// Default application service for the hosted authoring boundary.
#[derive(Clone)]
pub struct WorkflowAuthoringApplicationService {
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
    journals: Arc<dyn IWorkflowAuthoringRepository>,
    flow: Arc<dyn IWorkflowAuthoringFlowPort>,
}

impl WorkflowAuthoringApplicationService {
    /// Construct the service from the Workflow identity repository, journal
    /// store, and Flow-owned validation/application adapter.
    pub fn new(
        workflows: Arc<dyn IWorkflowDefinitionRepository>,
        journals: Arc<dyn IWorkflowAuthoringRepository>,
        flow: Arc<dyn IWorkflowAuthoringFlowPort>,
    ) -> Self {
        Self {
            workflows,
            journals,
            flow,
        }
    }

    async fn authorize(
        &self,
        key: WorkflowAuthoringJournalKey,
        access: &WorkflowAccess,
    ) -> ApplicationResult<()> {
        key.validate().map_err(ApplicationError::Invalid)?;
        let definition = resource_access::workflow_definition(
            self.workflows.as_ref(),
            key.organization_id,
            key.workflow_definition_id,
            access,
        )
        .await?;
        // The definition lookup is organization-scoped, while the journal key
        // additionally carries the project. Treat drift as not-found so a
        // caller cannot use a mismatched project identity to probe state.
        if definition.project_id != key.project_id {
            return Err(ApplicationError::NotFound(AUTHORING_NOT_FOUND.into()));
        }
        Ok(())
    }

    async fn read_authorized_journal(
        &self,
        key: WorkflowAuthoringJournalKey,
        access: &WorkflowAccess,
    ) -> ApplicationResult<WorkflowAuthoringJournal> {
        self.authorize(key, access).await?;
        let journal = self
            .journals
            .find(key)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(AUTHORING_NOT_FOUND.into()))?;
        journal.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "stored workflow authoring journal is invalid: {error}"
            ))
        })?;
        Ok(journal)
    }
}

#[async_trait]
impl IWorkflowAuthoringApplicationPort for WorkflowAuthoringApplicationService {
    async fn create_journal(
        &self,
        request: CreateWorkflowAuthoringJournalRequest,
    ) -> ApplicationResult<WorkflowAuthoringJournal> {
        request
            .validate_identity()
            .map_err(ApplicationError::Invalid)?;
        self.authorize(request.key, &request.access)
            .await?;
        request
            .initial_snapshot
            .validate()
            .map_err(map_authoring_input_error)?;
        let initial_snapshot = self
            .flow
            .validate_snapshot(&request.initial_snapshot)
            .await?;
        initial_snapshot.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "Flow returned an invalid initial workflow authoring snapshot: {error}"
            ))
        })?;
        let journal = self
            .journals
            .create_with_context(
                CreateWorkflowAuthoringJournal {
                    key: request.key,
                    initial_snapshot,
                },
                request.write_context(),
            )
            .await?;
        journal.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "authoring repository returned an invalid journal: {error}"
            ))
        })?;
        Ok(journal)
    }

    async fn append_operation(
        &self,
        request: AppendWorkflowAuthoringRequest,
    ) -> ApplicationResult<WorkflowAuthoringAppend> {
        request
            .validate_identity()
            .map_err(ApplicationError::Invalid)?;
        self.authorize(request.key, &request.access)
            .await?;
        request
            .operation
            .validate()
            .map_err(map_authoring_input_error)?;

        // Authorized retries are answered from the immutable operation index.
        // The bounded repository lookup keeps this hot path independent of
        // total journal length and Flow latency.
        if let Some(entry) = self
            .journals
            .find_operation(request.key, request.operation.operation_id())
            .await?
        {
            if entry.operation_digest() != request.operation.operation_digest() {
                return Err(map_authoring_input_error(
                    WorkflowAuthoringError::IdempotencyConflict {
                        operation_id: request.operation.operation_id().to_owned(),
                    },
                ));
            }
            return Ok(WorkflowAuthoringAppend {
                entry,
                replayed: true,
            });
        }

        let current_snapshot = self
            .journals
            .current_snapshot(request.key)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(AUTHORING_NOT_FOUND.into()))?;
        current_snapshot.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "stored workflow authoring current snapshot is invalid: {error}"
            ))
        })?;
        if request.operation.base_snapshot_digest() != current_snapshot.snapshot_digest() {
            return Err(ApplicationError::Conflict(format!(
                "workflow authoring base digest mismatch: expected {}, received {}",
                current_snapshot.snapshot_digest(),
                request.operation.base_snapshot_digest()
            )));
        }

        let result_snapshot = self
            .flow
            .apply_operation(&current_snapshot, &request.operation)
            .await?;
        result_snapshot.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "Flow returned an invalid workflow authoring snapshot: {error}"
            ))
        })?;
        let context = request.write_context();
        Ok(self
            .journals
            .append_with_context(
                AppendWorkflowAuthoringOperation {
                    key: request.key,
                    operation: request.operation,
                    result_snapshot,
                },
                context,
            )
            .await?)
    }

    async fn get_journal(
        &self,
        request: GetWorkflowAuthoringJournalRequest,
    ) -> ApplicationResult<WorkflowAuthoringJournal> {
        request
            .validate_identity()
            .map_err(ApplicationError::Invalid)?;
        self.read_authorized_journal(request.key, &request.access)
            .await
    }

    async fn page_operations(
        &self,
        request: PageWorkflowAuthoringRequest,
    ) -> ApplicationResult<WorkflowAuthoringPage> {
        request
            .validate_identity()
            .map_err(ApplicationError::Invalid)?;
        self.authorize(request.key, &request.access)
            .await?;
        if !(1..=WORKFLOW_AUTHORING_MAX_PAGE_SIZE).contains(&request.limit) {
            return Err(ApplicationError::Invalid(format!(
                "workflow authoring page limit must be between 1 and {WORKFLOW_AUTHORING_MAX_PAGE_SIZE}"
            )));
        }
        self.journals
            .page(request.key, request.after_sequence, request.limit)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(AUTHORING_NOT_FOUND.into()))
    }
}

fn map_authoring_input_error(error: WorkflowAuthoringError) -> ApplicationError {
    match error {
        WorkflowAuthoringError::Invalid(message) => ApplicationError::Invalid(message),
        WorkflowAuthoringError::CasConflict { .. }
        | WorkflowAuthoringError::IdempotencyConflict { .. }
        | WorkflowAuthoringError::SequenceExhausted => {
            ApplicationError::Conflict(error.to_string())
        }
        WorkflowAuthoringError::InvalidPageLimit { .. }
        | WorkflowAuthoringError::InvalidCursor(_) => ApplicationError::Invalid(error.to_string()),
    }
}
