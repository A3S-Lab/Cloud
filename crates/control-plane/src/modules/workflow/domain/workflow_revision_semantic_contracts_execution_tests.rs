use super::*;
use crate::modules::workflow::domain::{CapabilityOwner, CapabilityReference};

fn execution_contracts(
    descriptor_id: &str,
    semantic_profile: &str,
) -> (WorkflowSpec, WorkflowRevisionSemanticContracts) {
    let mut execution_descriptor = descriptor(
        descriptor_id,
        WorkflowStepKind::Execution,
        "input",
        "result",
    );
    execution_descriptor.owner = WorkflowStepOwner::Executions;
    execution_descriptor.semantic_profile = semantic_profile.into();
    execution_descriptor.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
    execution_descriptor.required_bindings = vec![
        WorkflowStepBindingKind::CapabilityReference,
        WorkflowStepBindingKind::PlacementPolicy,
    ];
    execution_descriptor.allowed_capability_types = vec![CapabilityType::ExecutionTemplate];

    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.execution-dispatch".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            execution_descriptor,
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
            ),
        ],
    })
    .expect("execution dispatch registry");

    let mut execution = step("execute", WorkflowStepKind::Execution);
    execution.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Executions,
        capability_type: CapabilityType::ExecutionTemplate,
        resource_id: uuid::Uuid::now_v7(),
        revision: uuid::Uuid::now_v7().to_string(),
        digest: digest('e'),
        capability: "execution.run".into(),
    });
    let workflow = WorkflowSpec {
        name: "Execution dispatch authority".into(),
        description: String::new(),
        steps: vec![
            step("input", WorkflowStepKind::Input),
            execution,
            step("output", WorkflowStepKind::Output),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-execute".into(),
                source: "input".into(),
                target: "execute".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "execute-output".into(),
                source: "execute".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "support.execution-dispatch".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: [
            ("input", "workflow.input"),
            ("execute", descriptor_id),
            ("output", "workflow.output"),
        ]
        .into_iter()
        .map(
            |(step_id, bound_descriptor_id)| WorkflowStepDescriptorBinding {
                step_id: step_id.into(),
                descriptor_id: bound_descriptor_id.into(),
                descriptor_revision: "1.0.0".into(),
                semantic_digest: registry
                    .resolve(bound_descriptor_id, "1.0.0")
                    .expect("bound execution descriptor")
                    .semantic_digest()
                    .clone(),
            },
        )
        .collect(),
    })
    .expect("execution dispatch bindings");
    let contracts = WorkflowRevisionSemanticContracts::create(
        &workflow,
        bindings,
        registry,
        variable_contract(),
    )
    .expect("execution dispatch semantic contracts");
    (workflow, contracts)
}

#[test]
fn runtime_dispatch_requires_the_exact_finite_execution_profile() {
    let (finite_workflow, finite_contracts) =
        execution_contracts("executions.finite", "executions.finite");
    finite_contracts
        .validate_runtime_dispatch_support(&finite_workflow)
        .expect("exact finite Execution dispatch");

    for (descriptor_id, semantic_profile) in [
        ("execution.code", "execution.code"),
        ("executions.finite", "execution.code"),
        ("execution.code", "executions.finite"),
    ] {
        let (workflow, contracts) = execution_contracts(descriptor_id, semantic_profile);
        let error = contracts
            .validate_runtime_dispatch_support(&workflow)
            .expect_err("non-finite Execution profile must remain fenced");
        assert!(
            error.contains("has no admitted Cloud runtime dispatch port"),
            "unexpected Execution dispatch admission error for {descriptor_id}/{semantic_profile}: {error}"
        );
    }
}
