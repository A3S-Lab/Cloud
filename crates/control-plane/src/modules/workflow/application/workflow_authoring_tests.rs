use super::{
    AppendWorkflowAuthoringRequest, CreateWorkflowAuthoringJournalRequest,
    GetWorkflowAuthoringJournalRequest, IWorkflowAuthoringApplicationPort,
    IWorkflowAuthoringFlowPort, PageWorkflowAuthoringRequest, WorkflowAuthoringApplicationService,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{
    AppendWorkflowAuthoringOperation, CreateWorkflowAuthoringJournal,
    CreateWorkflowDefinitionWrite, IWorkflowAuthoringRepository, IWorkflowDefinitionRepository,
    ReviseWorkflowDefinitionWrite, WorkflowAuthoringAppend, WorkflowAuthoringEntry,
    WorkflowAuthoringJournal, WorkflowAuthoringJournalKey, WorkflowAuthoringOperation,
    WorkflowAuthoringPage, WorkflowAuthoringSnapshot, WorkflowDefinition, WorkflowDefinitionRecord,
    WorkflowRevision,
};
use crate::modules::workflow::InMemoryWorkflowAuthoringRepository;
use async_trait::async_trait;
use chrono::Utc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;
use crate::modules::workflow::application::WorkflowAccess;

struct DefinitionRepository {
    definition: WorkflowDefinition,
}

#[async_trait]
impl IWorkflowDefinitionRepository for DefinitionRepository {
    async fn create(
        &self,
        _write: CreateWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        Err(RepositoryError::Storage("unused test operation".into()))
    }

    async fn revise(
        &self,
        _write: ReviseWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        Err(RepositoryError::Storage("unused test operation".into()))
    }

    async fn replay(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<WorkflowDefinitionRecord>, RepositoryError> {
        Ok(None)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Option<WorkflowDefinition>, RepositoryError> {
        Ok((organization_id == self.definition.organization_id
            && definition_id == self.definition.id)
            .then(|| self.definition.clone()))
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
    ) -> Result<Vec<WorkflowDefinition>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn find_revision(
        &self,
        _organization_id: OrganizationId,
        _definition_id: WorkflowDefinitionId,
        _revision_id: WorkflowRevisionId,
    ) -> Result<Option<WorkflowRevision>, RepositoryError> {
        Ok(None)
    }

    async fn list_revisions(
        &self,
        _organization_id: OrganizationId,
        _definition_id: WorkflowDefinitionId,
    ) -> Result<Vec<WorkflowRevision>, RepositoryError> {
        Ok(Vec::new())
    }
}

struct RecordingFlow {
    allow_snapshots: bool,
    result: WorkflowAuthoringSnapshot,
    validate_calls: AtomicUsize,
    apply_calls: AtomicUsize,
    last_base: Mutex<Option<Sha256Digest>>,
}

impl RecordingFlow {
    fn new(allow_snapshots: bool, result: WorkflowAuthoringSnapshot) -> Self {
        Self {
            allow_snapshots,
            result,
            validate_calls: AtomicUsize::new(0),
            apply_calls: AtomicUsize::new(0),
            last_base: Mutex::new(None),
        }
    }
}

#[async_trait]
impl IWorkflowAuthoringFlowPort for RecordingFlow {
    async fn validate_snapshot(
        &self,
        _snapshot: &WorkflowAuthoringSnapshot,
    ) -> ApplicationResult<WorkflowAuthoringSnapshot> {
        self.validate_calls.fetch_add(1, Ordering::SeqCst);
        if self.allow_snapshots {
            Ok(_snapshot.clone())
        } else {
            Err(ApplicationError::Invalid(
                "Flow rejected the workflow snapshot".into(),
            ))
        }
    }

    async fn apply_operation(
        &self,
        base_snapshot: &WorkflowAuthoringSnapshot,
        _operation: &WorkflowAuthoringOperation,
    ) -> ApplicationResult<WorkflowAuthoringSnapshot> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        *self.last_base.lock().await = Some(base_snapshot.snapshot_digest().clone());
        if self.allow_snapshots {
            Ok(self.result.clone())
        } else {
            Err(ApplicationError::Invalid(
                "Flow rejected the workflow operation".into(),
            ))
        }
    }
}

struct Fixture {
    service: WorkflowAuthoringApplicationService,
    flow: Arc<RecordingFlow>,
    key: crate::modules::workflow::domain::WorkflowAuthoringJournalKey,
    initial: WorkflowAuthoringSnapshot,
}

fn fixture(allow_snapshots: bool) -> Fixture {
    fixture_with_repository(
        allow_snapshots,
        Arc::new(InMemoryWorkflowAuthoringRepository::new()),
    )
}

fn fixture_with_repository(
    allow_snapshots: bool,
    journals: Arc<dyn IWorkflowAuthoringRepository>,
) -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        WorkflowDefinitionId::new(),
        "authoring-test".into(),
        String::new(),
        WorkflowRevisionId::new(),
        Sha256Digest::from_bytes(b"definition"),
        crate::modules::shared_kernel::domain::PrincipalId::new(),
        Utc::now(),
    )
    .expect("definition");
    let key = crate::modules::workflow::domain::WorkflowAuthoringJournalKey::new(
        organization_id,
        project_id,
        definition.id,
    );
    let initial = snapshot(b"initial");
    let flow = Arc::new(RecordingFlow::new(allow_snapshots, snapshot(b"next")));
    let service = WorkflowAuthoringApplicationService::new(
        Arc::new(DefinitionRepository { definition }),
        journals,
        Arc::clone(&flow) as Arc<dyn IWorkflowAuthoringFlowPort>,
    );
    Fixture {
        service,
        flow,
        key,
        initial,
    }
}

/// Guard the application append path against accidentally reintroducing an
/// unbounded full-journal read. The optimized head/index methods remain
/// available to the service, while explicit `get_journal` is intentionally
/// rejected by this test adapter.
struct NoFullJournalReadRepository {
    inner: InMemoryWorkflowAuthoringRepository,
}

#[async_trait]
impl IWorkflowAuthoringRepository for NoFullJournalReadRepository {
    async fn create(
        &self,
        write: CreateWorkflowAuthoringJournal,
    ) -> Result<WorkflowAuthoringJournal, RepositoryError> {
        self.inner.create(write).await
    }

    async fn append(
        &self,
        write: AppendWorkflowAuthoringOperation,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError> {
        self.inner.append(write).await
    }

    async fn current_snapshot(
        &self,
        key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringSnapshot>, RepositoryError> {
        self.inner.current_snapshot(key).await
    }

    async fn find_operation(
        &self,
        key: WorkflowAuthoringJournalKey,
        operation_id: &str,
    ) -> Result<Option<WorkflowAuthoringEntry>, RepositoryError> {
        self.inner.find_operation(key, operation_id).await
    }

    async fn find(
        &self,
        _key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringJournal>, RepositoryError> {
        Err(RepositoryError::Storage(
            "full workflow authoring journal reads are disabled on the append path".into(),
        ))
    }

    async fn page(
        &self,
        key: WorkflowAuthoringJournalKey,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Option<WorkflowAuthoringPage>, RepositoryError> {
        self.inner.page(key, after_sequence, limit).await
    }
}

fn snapshot(bytes: &[u8]) -> WorkflowAuthoringSnapshot {
    WorkflowAuthoringSnapshot::try_from_bytes(bytes.to_vec()).expect("snapshot")
}

fn operation(id: &str, base: &WorkflowAuthoringSnapshot) -> WorkflowAuthoringOperation {
    WorkflowAuthoringOperation::try_new(
        id,
        base.snapshot_digest().clone(),
        format!("operation:{id}").into_bytes(),
    )
    .expect("operation")
}

fn actor() -> PrincipalId {
    PrincipalId::new()
}

fn request_id() -> Uuid {
    Uuid::now_v7()
}

#[tokio::test]
async fn authoring_service_validates_once_and_replays_without_reapplying_flow() {
    let fixture = fixture(true);
    fixture
        .service
        .create_journal(CreateWorkflowAuthoringJournalRequest {
            key: fixture.key,
            initial_snapshot: fixture.initial.clone(),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await
        .expect("create journal");

    let first = fixture
        .service
        .append_operation(AppendWorkflowAuthoringRequest {
            key: fixture.key,
            operation: operation("op-1", &fixture.initial),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await
        .expect("append operation");
    assert!(!first.replayed);
    assert_eq!(first.entry.sequence(), 1);
    assert_eq!(fixture.flow.validate_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.flow.apply_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.flow.last_base.lock().await,
        Some(fixture.initial.snapshot_digest().clone())
    );

    let replay = fixture
        .service
        .append_operation(AppendWorkflowAuthoringRequest {
            key: fixture.key,
            operation: operation("op-1", &fixture.initial),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await
        .expect("replay operation");
    assert!(replay.replayed);
    assert_eq!(replay.entry, first.entry);
    assert_eq!(fixture.flow.apply_calls.load(Ordering::SeqCst), 1);

    let journal = fixture
        .service
        .get_journal(GetWorkflowAuthoringJournalRequest {
            key: fixture.key,
            access: WorkflowAccess::organization_wide(),
        })
        .await
        .expect("get journal");
    assert_eq!(journal.last_sequence(), 1);
    assert_eq!(journal.current_snapshot(), &snapshot(b"next"));

    let page = fixture
        .service
        .page_operations(PageWorkflowAuthoringRequest {
            key: fixture.key,
            after_sequence: None,
            limit: 10,
            access: WorkflowAccess::organization_wide(),
        })
        .await
        .expect("page operations");
    assert_eq!(page.entries, vec![first.entry]);
    assert_eq!(page.next_cursor, None);
}

#[tokio::test]
async fn append_path_uses_head_and_operation_index_without_full_journal_read() {
    let fixture = fixture_with_repository(
        true,
        Arc::new(NoFullJournalReadRepository {
            inner: InMemoryWorkflowAuthoringRepository::new(),
        }),
    );
    fixture
        .service
        .create_journal(CreateWorkflowAuthoringJournalRequest {
            key: fixture.key,
            initial_snapshot: fixture.initial.clone(),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await
        .expect("create journal");

    let appended = fixture
        .service
        .append_operation(AppendWorkflowAuthoringRequest {
            key: fixture.key,
            operation: operation("bounded", &fixture.initial),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await
        .expect("append operation");
    assert!(!appended.replayed);
    assert_eq!(appended.entry.sequence(), 1);

    let full_read = fixture
        .service
        .get_journal(GetWorkflowAuthoringJournalRequest {
            key: fixture.key,
            access: WorkflowAccess::organization_wide(),
        })
        .await;
    assert!(matches!(
        full_read,
        Err(ApplicationError::Internal(message))
            if message.contains("full workflow authoring journal reads")
    ));
}

#[tokio::test]
async fn authorization_and_cas_checks_happen_before_flow_application() {
    let fixture = fixture(true);
    let denied = fixture
        .service
        .create_journal(CreateWorkflowAuthoringJournalRequest {
            key: fixture.key,
            initial_snapshot: fixture.initial.clone(),
            access: WorkflowAccess::restricted([]),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await;
    assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    assert_eq!(fixture.flow.validate_calls.load(Ordering::SeqCst), 0);

    fixture
        .service
        .create_journal(CreateWorkflowAuthoringJournalRequest {
            key: fixture.key,
            initial_snapshot: fixture.initial.clone(),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await
        .expect("create journal");
    let stale_base = snapshot(b"stale");
    let stale = fixture
        .service
        .append_operation(AppendWorkflowAuthoringRequest {
            key: fixture.key,
            operation: operation("stale", &stale_base),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await;
    assert!(matches!(stale, Err(ApplicationError::Conflict(_))));
    assert_eq!(fixture.flow.apply_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn flow_rejection_is_not_written_and_page_limits_are_checked() {
    let fixture = fixture(false);
    let rejected = fixture
        .service
        .create_journal(CreateWorkflowAuthoringJournalRequest {
            key: fixture.key,
            initial_snapshot: fixture.initial.clone(),
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: actor(),
            request_id: request_id(),
        })
        .await;
    assert!(matches!(rejected, Err(ApplicationError::Invalid(_))));

    let invalid_limit = fixture
        .service
        .page_operations(PageWorkflowAuthoringRequest {
            key: fixture.key,
            after_sequence: None,
            limit: 0,
            access: WorkflowAccess::organization_wide(),
        })
        .await;
    assert!(matches!(invalid_limit, Err(ApplicationError::Invalid(_))));
}

#[tokio::test]
async fn invalid_audit_context_is_rejected_before_authorization_or_flow() {
    let fixture = fixture(true);
    let result = fixture
        .service
        .create_journal(CreateWorkflowAuthoringJournalRequest {
            key: fixture.key,
            initial_snapshot: fixture.initial,
            access: WorkflowAccess::organization_wide(),
            actor_principal_id: PrincipalId::from_uuid(Uuid::nil()),
            request_id: Uuid::nil(),
        })
        .await;
    assert!(
        matches!(result, Err(ApplicationError::Invalid(message)) if message.contains("write context"))
    );
    assert_eq!(fixture.flow.validate_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn project_identity_drift_is_hidden_as_not_found() {
    let fixture = fixture(true);
    let wrong_key = crate::modules::workflow::domain::WorkflowAuthoringJournalKey::new(
        fixture.key.organization_id,
        ProjectId::new(),
        fixture.key.workflow_definition_id,
    );
    let result = fixture
        .service
        .get_journal(GetWorkflowAuthoringJournalRequest {
            key: wrong_key,
            access: WorkflowAccess::organization_wide(),
        })
        .await;
    assert!(matches!(result, Err(ApplicationError::NotFound(_))));
}

#[test]
fn authoring_application_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowAuthoringApplicationService>();
    assert_send_sync::<CreateWorkflowAuthoringJournalRequest>();
    assert_send_sync::<AppendWorkflowAuthoringRequest>();
    assert_send_sync::<GetWorkflowAuthoringJournalRequest>();
    assert_send_sync::<PageWorkflowAuthoringRequest>();
}
