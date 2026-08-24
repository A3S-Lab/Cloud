use super::{
    IWorkflowDefinitionPublicationPort, WorkflowDefinitionPublicationProvenance,
    WorkflowDefinitionPublicationRequest, WorkflowDefinitionPublicationService,
};
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::InMemoryWorkflowDefinitionRepository;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn project_admission_precedes_acl_parsing() {
    let publications = WorkflowDefinitionPublicationService::new(
        Arc::new(InMemoryProjectsRepository::new()),
        Arc::new(InMemoryWorkflowDefinitionRepository::new()),
    );
    let result = publications
        .publish(WorkflowDefinitionPublicationRequest {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            definition_id: WorkflowDefinitionId::new(),
            revision_id: WorkflowRevisionId::new(),
            definition_acl: "not valid A3S ACL".into(),
            payloads: Vec::new(),
            semantic_contracts: None,
            provenance: WorkflowDefinitionPublicationProvenance::UserAuthored,
            actor_principal_id: PrincipalId::new(),
            idempotency_scope: "workflow-publication-test".into(),
            idempotency_key: "missing-project".into(),
            request_id: Uuid::now_v7(),
        })
        .await;

    assert_eq!(
        result,
        Err(ApplicationError::NotFound("project not found".into()))
    );
}

#[test]
fn publication_port_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowDefinitionPublicationProvenance>();
    assert_send_sync::<WorkflowDefinitionPublicationRequest>();
    assert_send_sync::<WorkflowDefinitionPublicationService>();
}
