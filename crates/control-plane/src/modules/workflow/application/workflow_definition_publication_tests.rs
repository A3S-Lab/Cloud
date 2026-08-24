use super::{
    validate_user_authored_runtime_support, IWorkflowDefinitionPublicationPort,
    WorkflowDefinitionPublicationProvenance, WorkflowDefinitionPublicationRequest,
    WorkflowDefinitionPublicationService,
};
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::{
    CapabilityReference, CapabilityType, InMemoryWorkflowDefinitionRepository, WorkflowContract,
    WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind, WorkflowStepSpec,
};
use std::sync::Arc;
use uuid::Uuid;

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn legacy_provider_contract(
    kind: WorkflowStepKind,
    capability_type: CapabilityType,
) -> WorkflowContract {
    let step = |id: &str, kind: WorkflowStepKind| WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest: digest('a'),
        input_schema_digest: digest('b'),
        output_schema_digest: digest('c'),
        policy_digest: None,
        capability: None,
    };
    let mut provider = step("provider", kind);
    provider.capability = Some(CapabilityReference {
        owner: capability_type.owner(),
        capability_type,
        resource_id: Uuid::now_v7(),
        revision: "release-1".into(),
        digest: digest('d'),
        capability: format!("{}.invoke", kind.as_str()),
    });
    WorkflowContract::from_spec(WorkflowSpec {
        name: "Legacy provider".into(),
        description: "Structurally valid legacy provider graph".into(),
        steps: vec![
            step("input", WorkflowStepKind::Input),
            provider,
            step("output", WorkflowStepKind::Output),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-provider".into(),
                source: "input".into(),
                target: "provider".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "provider-output".into(),
                source: "provider".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    })
    .expect("structurally valid legacy provider contract")
}

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
fn new_user_publication_rejects_unwired_legacy_provider_kinds() {
    let cases = [
        (WorkflowStepKind::Agent, CapabilityType::AgentRelease),
        (WorkflowStepKind::Mcp, CapabilityType::McpServiceProfile),
        (WorkflowStepKind::Model, CapabilityType::ModelRevision),
        (WorkflowStepKind::Tool, CapabilityType::UsePackage),
        (WorkflowStepKind::Memory, CapabilityType::UsePackage),
        (
            WorkflowStepKind::Subworkflow,
            CapabilityType::WorkflowRevision,
        ),
    ];

    for (kind, capability_type) in cases {
        let contract = legacy_provider_contract(kind, capability_type);
        let error = validate_user_authored_runtime_support(&contract, None)
            .expect_err("unwired legacy provider publication must fail closed");
        assert!(
            error.contains("has no admitted Cloud runtime dispatch port"),
            "unexpected {kind:?} admission error: {error}"
        );
    }
}

#[test]
fn publication_port_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowDefinitionPublicationProvenance>();
    assert_send_sync::<WorkflowDefinitionPublicationRequest>();
    assert_send_sync::<WorkflowDefinitionPublicationService>();
}
