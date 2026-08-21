use super::*;
use crate::modules::workflow::domain::{
    CapabilityOwner, CapabilityReference, WorkflowStepDescriptorAdmission,
    WorkflowStepFailureContract, WorkflowStepFallbackMode, WorkflowStepKind,
    WorkflowStepPresentationSpec,
};
use uuid::Uuid;

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn step() -> super::super::WorkflowStepSpec {
    super::super::WorkflowStepSpec {
        id: "invoke".into(),
        label: "Invoke".into(),
        kind: WorkflowStepKind::Service,
        configuration_digest: digest('a'),
        input_schema_digest: digest('b'),
        output_schema_digest: digest('c'),
        policy_digest: Some(digest('d')),
        capability: Some(CapabilityReference {
            owner: CapabilityOwner::Connectors,
            capability_type: CapabilityType::ConnectorRevision,
            resource_id: Uuid::now_v7(),
            revision: Uuid::now_v7().to_string(),
            digest: digest('e'),
            capability: "connector.http".into(),
        }),
    }
}

fn descriptor() -> super::super::WorkflowStepDescriptorSpec {
    super::super::WorkflowStepDescriptorSpec {
        id: "connector.http".into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Connectors,
        kind: Some(WorkflowStepKind::Service),
        semantic_profile: "connector.http".into(),
        execution_class: WorkflowStepExecutionClass::OwningApplicationPort,
        input_ports: Vec::new(),
        output_ports: Vec::new(),
        configuration_schema_digest: digest('a'),
        default_policy_digest: None,
        required_bindings: vec![WorkflowStepBindingKind::CapabilityReference],
        allowed_capability_types: vec![CapabilityType::ConnectorRevision],
        failure: WorkflowStepFailureContract {
            error_output: None,
            retry_classification: WorkflowStepRetryClassification::OwnerClassified,
            fallback: WorkflowStepFallbackMode::Unsupported,
            failure_branch: false,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 2,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: "HTTP Request".into(),
            summary: "Calls one exact Connector revision".into(),
            icon_key: "connector.http".into(),
        },
    }
}

#[test]
fn connector_retry_classification_stays_with_the_connectors_owner() {
    let step = step();
    let descriptor = descriptor();
    validate_connector_retry_authority(&step, &descriptor).expect("connector authority");

    let mut drifted = descriptor.clone();
    drifted.owner = WorkflowStepOwner::Workflow;
    assert!(validate_connector_retry_authority(&step, &drifted).is_err());
    drifted = descriptor.clone();
    drifted.semantic_profile = "service.http".into();
    assert!(validate_connector_retry_authority(&step, &drifted).is_err());
    drifted = descriptor;
    drifted.failure.retry_classification = WorkflowStepRetryClassification::FlowRetryable;
    assert!(validate_connector_retry_authority(&step, &drifted).is_err());
}

#[test]
fn capability_free_service_requires_the_exact_application_variable_descriptor() {
    let mut candidate = step();
    candidate.capability = None;
    candidate.policy_digest = None;

    let mut generic = descriptor();
    generic.id = "workflow.service".into();
    generic.semantic_profile = "workflow.service".into();
    generic.owner = WorkflowStepOwner::Workflow;
    generic.required_bindings.clear();
    generic.allowed_capability_types.clear();
    generic.failure.retry_classification = WorkflowStepRetryClassification::NotRetryable;
    assert!(validate_capability_binding(&candidate, &generic).is_err());

    generic.id = "application.conversation-variable-assign".into();
    generic.semantic_profile = "application.conversation-variable-assign".into();
    generic.owner = WorkflowStepOwner::Applications;
    generic.required_bindings = vec![WorkflowStepBindingKind::ReleaseReference];
    validate_capability_binding(&candidate, &generic)
        .expect("exact descriptor-bound Application variable Service");
}
