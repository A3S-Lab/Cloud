use a3s_cloud_control_plane::modules::shared_kernel::domain::Sha256Digest;
use a3s_cloud_control_plane::modules::workflow::{
    CapabilityType, WorkflowDataType, WorkflowStepBindingKind, WorkflowStepDescriptorAdmission,
    WorkflowStepDescriptorRegistry, WorkflowStepDescriptorRegistrySpec, WorkflowStepDescriptorSpec,
    WorkflowStepExecutionClass, WorkflowStepFailureContract, WorkflowStepFallbackMode,
    WorkflowStepKind, WorkflowStepOwner, WorkflowStepPort, WorkflowStepPortCardinality,
    WorkflowStepPresentationSpec, WorkflowStepRetryClassification, WorkflowStepSpec,
};

pub(super) fn workflow_step(
    id: &str,
    kind: WorkflowStepKind,
    configuration_digest: Sha256Digest,
    schema_digest: Sha256Digest,
) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest,
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest,
        policy_digest: None,
        capability: None,
    }
}

pub(super) fn descriptor_registry(
    configuration_schema_digest: Sha256Digest,
    composite_configuration_schema_digest: Option<Sha256Digest>,
) -> WorkflowStepDescriptorRegistry {
    let mut descriptors = vec![
        descriptor(
            "workflow.input",
            WorkflowStepKind::Input,
            "invocation",
            "value",
            configuration_schema_digest.clone(),
        ),
        descriptor(
            "workflow.output",
            WorkflowStepKind::Output,
            "result",
            "value",
            configuration_schema_digest,
        ),
    ];
    if let Some(configuration_digest) = composite_configuration_schema_digest {
        let mut iteration = descriptor(
            "workflow.iteration",
            WorkflowStepKind::Subworkflow,
            "items",
            "result",
            configuration_digest,
        );
        iteration.execution_class = WorkflowStepExecutionClass::CompositeRegion;
        iteration.required_bindings = vec![WorkflowStepBindingKind::CapabilityReference];
        iteration.allowed_capability_types = vec![CapabilityType::WorkflowRevision];
        descriptors.push(iteration);
    }
    WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "integration.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors,
    })
    .expect("descriptor registry")
}

fn descriptor(
    id: &str,
    kind: WorkflowStepKind,
    input_port: &str,
    output_port: &str,
    configuration_schema_digest: Sha256Digest,
) -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: id.into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(kind),
        semantic_profile: id.into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports: vec![port(input_port)],
        output_ports: vec![port(output_port)],
        configuration_schema_digest,
        default_policy_digest: None,
        required_bindings: Vec::new(),
        allowed_capability_types: Vec::new(),
        failure: WorkflowStepFailureContract {
            error_output: None,
            retry_classification: WorkflowStepRetryClassification::NotRetryable,
            fallback: WorkflowStepFallbackMode::Unsupported,
            failure_branch: false,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 2,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: id.into(),
            summary: format!("{id} descriptor"),
            icon_key: id.into(),
        },
    }
}

fn port(name: &str) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type: WorkflowDataType::Object,
        cardinality: WorkflowStepPortCardinality::Single,
        required: true,
        dynamic: false,
    }
}
