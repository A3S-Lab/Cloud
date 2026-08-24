use super::*;
use crate::modules::shared_kernel::domain::Sha256Digest;

const REGISTRY_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.3/step-descriptor-registry.acl"
));

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn port(name: &str, value_type: WorkflowDataType) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type,
        cardinality: WorkflowStepPortCardinality::Single,
        required: true,
        dynamic: false,
    }
}

fn presentation(label: &str) -> WorkflowStepPresentationSpec {
    WorkflowStepPresentationSpec {
        label: label.into(),
        summary: format!("{label} descriptor"),
        icon_key: "workflow.input".into(),
    }
}

fn input_descriptor(label: &str) -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: "workflow.input".into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(WorkflowStepKind::Input),
        semantic_profile: "workflow.input".into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports: vec![port("invocation", WorkflowDataType::Object)],
        output_ports: vec![port("value", WorkflowDataType::Object)],
        configuration_schema_digest: digest('a'),
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
        maximum_compiler_schema_version: 3,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: presentation(label),
    }
}

fn execution_descriptor() -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: "executions.finite".into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Executions,
        kind: Some(WorkflowStepKind::Execution),
        semantic_profile: "executions.finite".into(),
        execution_class: WorkflowStepExecutionClass::OwningApplicationPort,
        input_ports: vec![port("input", WorkflowDataType::Object)],
        output_ports: vec![port("result", WorkflowDataType::Object)],
        configuration_schema_digest: digest('b'),
        default_policy_digest: Some(digest('c')),
        required_bindings: vec![
            WorkflowStepBindingKind::CapabilityReference,
            WorkflowStepBindingKind::PlacementPolicy,
        ],
        allowed_capability_types: vec![CapabilityType::ExecutionTemplate],
        failure: WorkflowStepFailureContract {
            error_output: Some(port("error", WorkflowDataType::Object)),
            retry_classification: WorkflowStepRetryClassification::OwnerClassified,
            fallback: WorkflowStepFallbackMode::FailureBranch,
            failure_branch: true,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 2,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: "Finite Execution".into(),
            summary: "Runs one exact Executions-owned template".into(),
            icon_key: "executions.finite".into(),
        },
    }
}

fn transform_descriptor() -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: "workflow.transform".into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(WorkflowStepKind::Transform),
        semantic_profile: "workflow.transform".into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports: vec![port("current", WorkflowDataType::Object)],
        output_ports: vec![port("value", WorkflowDataType::Object)],
        configuration_schema_digest: digest('d'),
        default_policy_digest: None,
        required_bindings: Vec::new(),
        allowed_capability_types: Vec::new(),
        failure: WorkflowStepFailureContract {
            error_output: Some(port("error", WorkflowDataType::Object)),
            retry_classification: WorkflowStepRetryClassification::NotRetryable,
            fallback: WorkflowStepFallbackMode::FailureBranch,
            failure_branch: true,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 3,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: "Transform".into(),
            summary: "Evaluates one deterministic Workflow-local transform".into(),
            icon_key: "workflow.transform".into(),
        },
    }
}

fn output_descriptor() -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: "workflow.output".into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(WorkflowStepKind::Output),
        semantic_profile: "workflow.output".into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports: vec![port("current", WorkflowDataType::Object)],
        output_ports: vec![port("value", WorkflowDataType::Object)],
        configuration_schema_digest: digest('e'),
        default_policy_digest: None,
        required_bindings: Vec::new(),
        allowed_capability_types: Vec::new(),
        failure: WorkflowStepFailureContract {
            error_output: Some(port("error", WorkflowDataType::Object)),
            retry_classification: WorkflowStepRetryClassification::NotRetryable,
            fallback: WorkflowStepFallbackMode::FailureBranch,
            failure_branch: true,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 3,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: "Output".into(),
            summary: "Renders one deterministic Workflow-local output".into(),
            icon_key: "workflow.output".into(),
        },
    }
}

fn registry_spec() -> WorkflowStepDescriptorRegistrySpec {
    WorkflowStepDescriptorRegistrySpec {
        id: "cloud.builtin".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            execution_descriptor(),
            input_descriptor("User Input"),
            output_descriptor(),
            transform_descriptor(),
        ],
    }
}

#[test]
fn registry_is_canonical_digest_addressed_and_restorable() {
    let registry = WorkflowStepDescriptorRegistry::from_spec(registry_spec()).expect("registry");
    assert_eq!(registry.id(), "cloud.builtin");
    assert_eq!(registry.revision(), "1.0.0");
    assert_eq!(registry.compiler_schema_version(), 2);
    assert_eq!(registry.descriptors().len(), 4);
    assert!(registry.digest().as_str().starts_with("sha256:"));
    assert_eq!(
        WorkflowStepDescriptorRegistry::parse_acl(registry.canonical_acl()).expect("parsed"),
        registry
    );
    assert_eq!(
        WorkflowStepDescriptorRegistry::restore(
            registry.canonical_acl(),
            registry.digest().as_str()
        )
        .expect("restored"),
        registry
    );
    assert!(WorkflowStepDescriptorRegistry::restore(
        registry.canonical_acl(),
        &format!("sha256:{}", "f".repeat(64)),
    )
    .is_err());
    assert!(
        WorkflowStepDescriptorRegistry::parse_acl(&format!("\n{}", registry.canonical_acl()))
            .is_err()
    );
    assert_eq!(
        WorkflowStepDescriptorRegistry::parse_acl(&registry.canonical_acl().replace('\n', "\r\n"))
            .expect("CRLF registry"),
        registry
    );
    assert!(WorkflowStepDescriptorRegistry::parse_acl(
        &registry.canonical_acl().replacen('\n', "\r", 1)
    )
    .is_err());
    assert!(WorkflowStepDescriptorRegistry::parse_acl(&format!(
        "{}\r",
        registry.canonical_acl().replace('\n', "\r\n")
    ))
    .is_err());
}

#[test]
fn checked_in_registry_fixture_matches_the_domain_generator() {
    let generated =
        WorkflowStepDescriptorRegistry::from_spec(registry_spec()).expect("generated registry");
    assert_eq!(
        REGISTRY_FIXTURE.replace("\r\n", "\n"),
        generated.canonical_acl()
    );
    let parsed =
        WorkflowStepDescriptorRegistry::parse_acl(REGISTRY_FIXTURE).expect("registry fixture");
    assert_eq!(parsed, generated);
}

#[test]
fn checked_in_registry_rejects_schema_and_authority_drift() {
    let registry_fixture = REGISTRY_FIXTURE.replace("\r\n", "\n");
    let unknown_root_attribute = registry_fixture.replacen(
        "  compiler_schema_version = 2\n",
        "  compiler_schema_version = 2\n  unknown = true\n",
        1,
    );
    assert!(WorkflowStepDescriptorRegistry::parse_acl(&unknown_root_attribute).is_err());

    let unsupported_schema = registry_fixture.replacen(
        WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA,
        "cloud.workflow.step-descriptor-registry.v2",
        1,
    );
    assert!(WorkflowStepDescriptorRegistry::parse_acl(&unsupported_schema).is_err());

    let invented_mcp_owner =
        registry_fixture.replacen("    owner = \"executions\"\n", "    owner = \"mcp\"\n", 1);
    assert!(WorkflowStepDescriptorRegistry::parse_acl(&invented_mcp_owner).is_err());

    let floating_revision =
        registry_fixture.replacen("  revision = \"1.0.0\"\n", "  revision = \"latest\"\n", 1);
    assert!(WorkflowStepDescriptorRegistry::parse_acl(&floating_revision).is_err());

    let mut invocation = input_descriptor("Webhook Trigger");
    invocation.id = "automation.webhook".into();
    invocation.semantic_profile = "automation.webhook".into();
    invocation.owner = WorkflowStepOwner::Automations;
    invocation.kind = None;
    invocation.execution_class = WorkflowStepExecutionClass::InvocationOnly;
    let canonical_invocation =
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![invocation],
            ..registry_spec()
        })
        .expect("canonical invocation descriptor");
    let invocation_with_flow_kind = canonical_invocation.canonical_acl().replacen(
        "    execution_class = \"invocation_only\"\n",
        "    execution_class = \"invocation_only\"\n    kind = \"input\"\n",
        1,
    );
    assert!(WorkflowStepDescriptorRegistry::parse_acl(&invocation_with_flow_kind).is_err());
}

#[test]
fn presentation_and_admission_metadata_do_not_change_execution_semantics() {
    let first = WorkflowStepDescriptorRegistry::from_spec(registry_spec()).expect("first");
    let mut changed_presentation = registry_spec();
    changed_presentation.descriptors[1].presentation = presentation("Invocation Input");
    let second =
        WorkflowStepDescriptorRegistry::from_spec(changed_presentation).expect("presentation");

    let first_input = first.resolve("workflow.input", "1.0.0").expect("input");
    let second_input = second.resolve("workflow.input", "1.0.0").expect("input");
    assert_eq!(first_input.semantic_acl(), second_input.semantic_acl());
    assert_eq!(
        first_input.semantic_digest(),
        second_input.semantic_digest()
    );
    assert_ne!(
        first_input.presentation().digest(),
        second_input.presentation().digest()
    );
    assert_ne!(first.digest(), second.digest());

    let mut unavailable = registry_spec();
    unavailable.descriptors[1].admission = WorkflowStepDescriptorAdmission::Unavailable;
    unavailable.descriptors[1].unavailable_reason = Some("provider gate is not verified".into());
    let unavailable =
        WorkflowStepDescriptorRegistry::from_spec(unavailable).expect("unavailable registry");
    assert_eq!(
        first_input.semantic_digest(),
        unavailable
            .resolve("workflow.input", "1.0.0")
            .expect("input")
            .semantic_digest()
    );
    assert_ne!(first.digest(), unavailable.digest());
    assert!(unavailable
        .resolve_for_compiler("workflow.input", "1.0.0", 2)
        .expect_err("unavailable descriptor")
        .contains("provider gate is not verified"));
}

#[test]
fn lookup_requires_an_exact_revision_and_compiler_range() {
    let registry = WorkflowStepDescriptorRegistry::from_spec(registry_spec()).expect("registry");
    let descriptor = registry
        .resolve_for_compiler("workflow.input", "1.0.0", 3)
        .expect("compatible descriptor");
    assert_eq!(descriptor.id(), "workflow.input");
    assert!(descriptor.supports_compiler_schema_version(2));
    assert!(descriptor.supports_compiler_schema_version(3));
    assert!(!descriptor.supports_compiler_schema_version(4));
    assert!(registry.resolve("workflow.input", "latest").is_none());
    assert!(registry
        .resolve_for_compiler("workflow.input", "1.0.0", 4)
        .is_err());
}

#[test]
fn duplicate_identity_and_admission_drift_fail_closed() {
    let mut duplicate = registry_spec();
    duplicate.descriptors.push(input_descriptor("Duplicate"));
    assert!(WorkflowStepDescriptorRegistry::from_spec(duplicate).is_err());

    let mut admitted_reason = registry_spec();
    admitted_reason.descriptors[1].unavailable_reason = Some("should not exist".into());
    assert!(WorkflowStepDescriptorRegistry::from_spec(admitted_reason).is_err());

    let mut missing_reason = registry_spec();
    missing_reason.descriptors[1].admission = WorkflowStepDescriptorAdmission::Unavailable;
    assert!(WorkflowStepDescriptorRegistry::from_spec(missing_reason).is_err());

    let mut invalid_port = registry_spec();
    invalid_port.descriptors[1].output_ports[0].name = "nested.value".into();
    assert!(WorkflowStepDescriptorRegistry::from_spec(invalid_port).is_err());
}

#[test]
fn invocation_and_external_binding_authority_fail_closed() {
    let mut invocation = input_descriptor("Webhook");
    invocation.id = "automation.webhook".into();
    invocation.semantic_profile = "automation.webhook".into();
    invocation.execution_class = WorkflowStepExecutionClass::InvocationOnly;
    invocation.owner = WorkflowStepOwner::Automations;
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![invocation.clone()],
            ..registry_spec()
        })
        .is_err()
    );

    invocation.kind = None;
    WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        descriptors: vec![invocation.clone()],
        ..registry_spec()
    })
    .expect("Automations invocation descriptor");

    invocation.owner = WorkflowStepOwner::Workflow;
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![invocation],
            ..registry_spec()
        })
        .is_err()
    );

    let mut unbound = execution_descriptor();
    unbound
        .required_bindings
        .retain(|binding| *binding != WorkflowStepBindingKind::CapabilityReference);
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![unbound],
            ..registry_spec()
        })
        .is_err()
    );

    let mut mismatched = execution_descriptor();
    mismatched.allowed_capability_types = vec![CapabilityType::AgentRelease];
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![mismatched],
            ..registry_spec()
        })
        .is_err()
    );
}

#[test]
fn execution_classes_retain_their_owning_context_boundaries() {
    let mut composite = input_descriptor("Iteration");
    composite.id = "workflow.iteration".into();
    composite.semantic_profile = "workflow.iteration".into();
    composite.execution_class = WorkflowStepExecutionClass::CompositeRegion;
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![composite.clone()],
            ..registry_spec()
        })
        .is_err()
    );

    composite.kind = Some(WorkflowStepKind::Subworkflow);
    WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        descriptors: vec![composite],
        ..registry_spec()
    })
    .expect("Workflow-owned composite region");

    let mut application_port = execution_descriptor();
    application_port.owner = WorkflowStepOwner::Workflow;
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![application_port.clone()],
            ..registry_spec()
        })
        .is_err()
    );
    application_port.owner = WorkflowStepOwner::Automations;
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![application_port.clone()],
            ..registry_spec()
        })
        .is_err()
    );

    application_port.owner = WorkflowStepOwner::Agents;
    application_port.kind = Some(WorkflowStepKind::Model);
    application_port.allowed_capability_types = vec![CapabilityType::ModelRevision];
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![application_port],
            ..registry_spec()
        })
        .is_err()
    );

    let mut answer = input_descriptor("Answer");
    answer.id = "application.answer".into();
    answer.semantic_profile = "application.answer".into();
    answer.owner = WorkflowStepOwner::Applications;
    answer.kind = Some(WorkflowStepKind::Output);
    answer.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
    answer.required_bindings = vec![WorkflowStepBindingKind::ReleaseReference];
    answer.admission = WorkflowStepDescriptorAdmission::Unavailable;
    answer.unavailable_reason = Some("APP0.2 is not verified".into());
    WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        descriptors: vec![answer],
        ..registry_spec()
    })
    .expect("Applications-owned Answer port");

    let mut connector = execution_descriptor();
    connector.id = "connector.http".into();
    connector.semantic_profile = "connector.http".into();
    connector.owner = WorkflowStepOwner::Connectors;
    connector.kind = Some(WorkflowStepKind::Service);
    connector.allowed_capability_types = vec![CapabilityType::ConnectorRevision];
    connector.admission = WorkflowStepDescriptorAdmission::Unavailable;
    connector.unavailable_reason = Some("AUT0.5 is not verified".into());
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        descriptors: vec![connector],
        ..registry_spec()
    })
    .expect("Connectors-owned application port");
    let parsed = WorkflowStepDescriptorRegistry::parse_acl(registry.canonical_acl())
        .expect("Connectors owner round trip");
    assert_eq!(
        parsed.descriptors()[0].spec().owner,
        WorkflowStepOwner::Connectors
    );
}

#[test]
fn failure_branch_and_default_fallback_require_typed_outputs() {
    let mut missing_error = execution_descriptor();
    missing_error.failure.error_output = None;
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![missing_error],
            ..registry_spec()
        })
        .is_err()
    );

    let mut missing_default = input_descriptor("Input");
    missing_default.output_ports.clear();
    missing_default.failure.fallback = WorkflowStepFallbackMode::DefaultOutput;
    assert!(
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            descriptors: vec![missing_default],
            ..registry_spec()
        })
        .is_err()
    );
}

#[test]
fn descriptor_contracts_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowStepDescriptorRegistry>();
    assert_send_sync::<WorkflowStepDescriptorRevision>();
    assert_send_sync::<WorkflowStepPresentation>();
}
