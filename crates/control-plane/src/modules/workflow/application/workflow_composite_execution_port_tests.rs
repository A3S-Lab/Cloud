use super::{
    IWorkflowCompositeExecutionPort, WorkflowCompositeExecutionApplicationService,
    WorkflowCompositeExecutionRequest,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, IdempotentWrite, OntologyId, OntologyRevisionId, OrganizationId,
    PlanRevisionId, PrincipalId, ProjectId, RepositoryError, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    CreateOntologyWrite, CreateWorkflowDefinitionWrite, IOntologyRepository,
    IWorkflowDefinitionRepository, Ontology, OntologyContract, OntologyObjectType, OntologyRecord,
    OntologyRevision, OntologySpec, ReviseOntologyWrite, ReviseWorkflowDefinitionWrite,
    WorkflowCompositeFrame, WorkflowCompositeFrameMode, WorkflowContract, WorkflowDefinition,
    WorkflowDefinitionRecord, WorkflowRevision,
};
use crate::modules::workflow::infrastructure::persistence::{
    InMemoryWorkflowGoalRepository, InMemoryWorkflowRunRepository,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct StaticWorkflowRepository {
    definition: WorkflowDefinition,
    revision: WorkflowRevision,
}

#[async_trait]
impl IWorkflowDefinitionRepository for StaticWorkflowRepository {
    async fn create(
        &self,
        _write: CreateWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        Err(RepositoryError::Storage("read-only fixture".into()))
    }

    async fn revise(
        &self,
        _write: ReviseWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        Err(RepositoryError::Storage("read-only fixture".into()))
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Option<WorkflowDefinition>, RepositoryError> {
        Ok((self.definition.organization_id == organization_id
            && self.definition.id == definition_id)
            .then(|| self.definition.clone()))
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowDefinition>, RepositoryError> {
        Ok((self.definition.organization_id == organization_id
            && self.definition.project_id == project_id)
            .then(|| self.definition.clone())
            .into_iter()
            .collect())
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
        revision_id: WorkflowRevisionId,
    ) -> Result<Option<WorkflowRevision>, RepositoryError> {
        Ok((self.revision.organization_id == organization_id
            && self.revision.workflow_definition_id == definition_id
            && self.revision.id == revision_id)
            .then(|| self.revision.clone()))
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Vec<WorkflowRevision>, RepositoryError> {
        Ok((self.revision.organization_id == organization_id
            && self.revision.workflow_definition_id == definition_id)
            .then(|| self.revision.clone())
            .into_iter()
            .collect())
    }
}

struct StaticOntologyRepository {
    revision: OntologyRevision,
}

#[async_trait]
impl IOntologyRepository for StaticOntologyRepository {
    async fn create(
        &self,
        _write: CreateOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError> {
        Err(RepositoryError::Storage("read-only fixture".into()))
    }

    async fn revise(
        &self,
        _write: ReviseOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError> {
        Err(RepositoryError::Storage("read-only fixture".into()))
    }

    async fn find(
        &self,
        _organization_id: OrganizationId,
        _ontology_id: OntologyId,
    ) -> Result<Option<Ontology>, RepositoryError> {
        Ok(None)
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
    ) -> Result<Vec<Ontology>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
        revision_id: OntologyRevisionId,
    ) -> Result<Option<OntologyRevision>, RepositoryError> {
        Ok((self.revision.organization_id == organization_id
            && self.revision.ontology_id == ontology_id
            && self.revision.id == revision_id)
            .then(|| self.revision.clone()))
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
    ) -> Result<Vec<OntologyRevision>, RepositoryError> {
        Ok((self.revision.organization_id == organization_id
            && self.revision.ontology_id == ontology_id)
            .then(|| self.revision.clone())
            .into_iter()
            .collect())
    }
}

struct Fixture {
    request: WorkflowCompositeExecutionRequest,
    service: WorkflowCompositeExecutionApplicationService,
    goals: Arc<InMemoryWorkflowGoalRepository>,
    runs: Arc<InMemoryWorkflowRunRepository>,
}

fn fixture() -> Fixture {
    let template =
        crate::modules::workflow::test_support::workflow_run_input().expect("WorkflowRun template");
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let principal_id = PrincipalId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let now = crate::modules::workflow::test_support::timestamp(8, 0);
    let contract =
        WorkflowContract::from_spec(template.plan.workflow_spec().expect("Workflow spec"))
            .expect("Workflow contract");
    let payloads = template
        .payloads
        .iter()
        .map(|payload| payload.restore())
        .collect::<Result<Vec<_>, _>>()
        .expect("Workflow payloads");
    let revision = WorkflowRevision::initial(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        contract,
        payloads,
        principal_id,
        now,
    )
    .expect("Workflow revision");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        revision.contract.spec().name.clone(),
        revision.contract.spec().description.clone(),
        revision_id,
        revision.contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("Workflow definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Composite child ontology".into(),
        description: String::new(),
        object_types: vec![OntologyObjectType {
            id: "request".into(),
            label: "Request".into(),
            schema_digest: Sha256Digest::from_bytes(b"request schema"),
            key_fields: vec!["id".into()],
        }],
        relation_types: Vec::new(),
        rules: Vec::new(),
    })
    .expect("Ontology contract");
    let ontology = OntologyRevision::initial(
        organization_id,
        project_id,
        ontology_id,
        ontology_revision_id,
        ontology_contract,
        principal_id,
        now,
    );
    let child_input = json!({"ticketId": "T-42", "priority": "high"});
    let child_input_digest = Sha256Digest::from_bytes(
        &canonical_json_bounded(&child_input, 64 * 1024, "child input")
            .expect("canonical child input"),
    );
    let frame = WorkflowCompositeFrame {
        schema: crate::modules::workflow::domain::WORKFLOW_COMPOSITE_FRAME_SCHEMA.into(),
        organization_id,
        project_id,
        workflow_run_id: WorkflowRunId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest: Sha256Digest::from_bytes(b"parent plan"),
        variable_contract_digest: Sha256Digest::from_bytes(b"parent variables"),
        composite_regions_digest: Sha256Digest::from_bytes(b"parent regions"),
        region_step_id: "iteration".into(),
        mode: WorkflowCompositeFrameMode::Iteration,
        ordinal: 0,
        child_workflow_definition_id: definition_id,
        child_workflow_revision_id: revision_id,
        child_workflow_digest: revision.contract.digest().clone(),
        typed_projection_authoritative: false,
        child_input,
        child_input_digest,
        captured_variables: Default::default(),
        frame_digest: Sha256Digest::from_bytes(b"parent frame zero"),
    };
    let request = WorkflowCompositeExecutionRequest {
        frame,
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology.contract.digest().clone(),
        environment_id: None,
        requested_by: principal_id,
        requested_at: now,
        timeout_seconds: 3_600,
    };
    let goals = Arc::new(InMemoryWorkflowGoalRepository::new());
    let runs = Arc::new(InMemoryWorkflowRunRepository::new());
    let service = WorkflowCompositeExecutionApplicationService::new(
        Arc::new(StaticWorkflowRepository {
            definition,
            revision,
        }),
        Arc::new(StaticOntologyRepository { revision: ontology }),
        goals.clone(),
        runs.clone(),
    );
    Fixture {
        request,
        service,
        goals,
        runs,
    }
}

#[tokio::test]
async fn composite_child_start_is_deterministic_and_idempotent() {
    let fixture = fixture();
    let expected_run_id = fixture.request.workflow_run_id();
    let first = fixture
        .service
        .start_or_adopt(&fixture.request)
        .await
        .expect("create child");
    let replay = fixture
        .service
        .start_or_adopt(&fixture.request)
        .await
        .expect("adopt child");

    assert_eq!(first, replay);
    assert_eq!(first.run.id, expected_run_id);
    assert_eq!(
        first.run.execution_input.goal_input,
        fixture.request.frame.child_input
    );
    assert_eq!(fixture.goals.outbox_events().await.len(), 1);
    assert_eq!(fixture.runs.outbox_events().await.len(), 1);
    assert_eq!(
        fixture
            .service
            .adopt(&fixture.request)
            .await
            .expect("adopt")
            .expect("child"),
        first
    );
}

#[tokio::test]
async fn composite_child_adoption_fails_closed_on_authority_drift() {
    let fixture = fixture();
    fixture
        .service
        .start_or_adopt(&fixture.request)
        .await
        .expect("create child");
    let mut drifted = fixture.request.clone();
    drifted.frame.child_workflow_digest = Sha256Digest::from_bytes(b"drifted revision");

    assert!(fixture.service.adopt(&drifted).await.is_err());
    assert_eq!(fixture.runs.outbox_events().await.len(), 1);
}

#[tokio::test]
async fn composite_child_cancellation_replays_one_write() {
    let fixture = fixture();
    fixture
        .service
        .start_or_adopt(&fixture.request)
        .await
        .expect("create child");
    let cancellation_at = crate::modules::workflow::test_support::timestamp(8, 1);
    let first = fixture
        .service
        .request_cancellation(
            &fixture.request,
            Some("parent cancellation".into()),
            fixture.request.requested_by,
            cancellation_at,
        )
        .await
        .expect("cancel child")
        .expect("child");
    let replay = fixture
        .service
        .request_cancellation(
            &fixture.request,
            Some("parent cancellation".into()),
            fixture.request.requested_by,
            cancellation_at,
        )
        .await
        .expect("replay cancellation")
        .expect("child");

    assert_eq!(first, replay);
    assert_eq!(
        first.run.status,
        crate::modules::workflow::domain::WorkflowRunStatus::Cancelling
    );
    assert_eq!(fixture.runs.outbox_events().await.len(), 2);
}

#[test]
fn composite_execution_port_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowCompositeExecutionRequest>();
    assert_send_sync::<WorkflowCompositeExecutionApplicationService>();
}
