use super::*;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId,
    ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId,
    WorkflowRunId,
};
use chrono::Utc;
use serde_json::json;

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
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

fn descriptor(
    id: &str,
    kind: WorkflowStepKind,
    input_port: &str,
    output_port: &str,
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
        configuration_schema_digest: digest('c'),
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

fn workflow() -> WorkflowSpec {
    WorkflowSpec {
        name: "Bound workflow".into(),
        description: String::new(),
        steps: vec![
            step("input", WorkflowStepKind::Input),
            step("output", WorkflowStepKind::Output),
        ],
        edges: vec![WorkflowEdgeSpec {
            id: "input-output".into(),
            source: "input".into(),
            target: "output".into(),
            source_handle: None,
        }],
    }
}

fn step(id: &str, kind: WorkflowStepKind) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest: digest('d'),
        input_schema_digest: digest('a'),
        output_schema_digest: digest('a'),
        policy_digest: None,
        capability: None,
    }
}

fn registry() -> WorkflowStepDescriptorRegistry {
    registry_with_output_bindings(Vec::new())
}

fn registry_with_output_bindings(
    required_bindings: Vec<WorkflowStepBindingKind>,
) -> WorkflowStepDescriptorRegistry {
    let mut output = descriptor(
        "workflow.output",
        WorkflowStepKind::Output,
        "result",
        "value",
    );
    output.required_bindings = required_bindings;
    WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            output,
        ],
    })
    .expect("registry")
}

pub(super) fn variable_contract() -> WorkflowVariableContract {
    WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![WorkflowVariableDeclaration {
            name: "request".into(),
            scope: WorkflowVariableScope::InvocationInput,
            value_type: WorkflowDataType::Object,
            value_schema_digest: digest('a'),
            source_schema_digest: Some(digest('a')),
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required: true,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: None,
            default_value_digest: None,
        }],
        reads: vec![WorkflowVariableRead {
            id: "output-request".into(),
            variable: "request".into(),
            consumer_step_id: "output".into(),
            consumer_region_id: None,
            target_port: "result".into(),
            path: Vec::new(),
            expected_type: WorkflowDataType::Object,
            expected_schema_digest: digest('a'),
            required: true,
            mode: WorkflowVariableReadMode::DirectValue,
        }],
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .expect("variable contract")
}

fn bindings(registry: &WorkflowStepDescriptorRegistry) -> WorkflowStepDescriptorBindings {
    WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: [("input", "workflow.input"), ("output", "workflow.output")]
            .into_iter()
            .map(|(step_id, descriptor_id)| WorkflowStepDescriptorBinding {
                step_id: step_id.into(),
                descriptor_id: descriptor_id.into(),
                descriptor_revision: "1.0.0".into(),
                semantic_digest: registry
                    .resolve(descriptor_id, "1.0.0")
                    .expect("descriptor")
                    .semantic_digest()
                    .clone(),
            })
            .collect(),
    })
    .expect("bindings")
}

#[test]
fn new_user_publication_cannot_self_admit_unwired_provider_dispatch() {
    let cases = [
        (
            WorkflowStepKind::Agent,
            WorkflowStepOwner::Agents,
            CapabilityType::AgentRelease,
        ),
        (
            WorkflowStepKind::Mcp,
            WorkflowStepOwner::Assets,
            CapabilityType::McpServiceProfile,
        ),
        (
            WorkflowStepKind::Model,
            WorkflowStepOwner::Inference,
            CapabilityType::ModelRevision,
        ),
        (
            WorkflowStepKind::Tool,
            WorkflowStepOwner::Use,
            CapabilityType::UsePackage,
        ),
        (
            WorkflowStepKind::Memory,
            WorkflowStepOwner::Use,
            CapabilityType::UsePackage,
        ),
    ];

    for (kind, owner, capability_type) in cases {
        let descriptor_id = format!("provider.{}", kind.as_str());
        let mut provider_descriptor = descriptor(&descriptor_id, kind, "input", "result");
        provider_descriptor.owner = owner;
        provider_descriptor.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
        provider_descriptor.required_bindings = vec![WorkflowStepBindingKind::CapabilityReference];
        provider_descriptor.allowed_capability_types = vec![capability_type];

        let mut descriptor_specs = registry()
            .descriptors()
            .iter()
            .map(|revision| revision.spec().clone())
            .collect::<Vec<_>>();
        descriptor_specs.push(provider_descriptor);
        let provider_registry =
            WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
                id: "support.bound".into(),
                revision: "1.0.0".into(),
                compiler_schema_version: 2,
                descriptors: descriptor_specs,
            })
            .expect("provider registry");

        let mut provider_step = step("provider", kind);
        provider_step.capability = Some(CapabilityReference {
            owner: capability_type.owner(),
            capability_type,
            resource_id: uuid::Uuid::now_v7(),
            revision: "release-1".into(),
            digest: digest('e'),
            capability: format!("{}.invoke", kind.as_str()),
        });
        let mut provider_workflow = workflow();
        provider_workflow.steps.insert(1, provider_step);
        provider_workflow.edges = vec![
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
        ];
        let provider_bindings =
            WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
                id: "support.bound".into(),
                revision: "1.0.0".into(),
                compiler_schema_version: 2,
                bindings: [
                    ("input", "workflow.input"),
                    ("provider", descriptor_id.as_str()),
                    ("output", "workflow.output"),
                ]
                .into_iter()
                .map(
                    |(step_id, bound_descriptor_id)| WorkflowStepDescriptorBinding {
                        step_id: step_id.into(),
                        descriptor_id: bound_descriptor_id.into(),
                        descriptor_revision: "1.0.0".into(),
                        semantic_digest: provider_registry
                            .resolve(bound_descriptor_id, "1.0.0")
                            .expect("bound descriptor")
                            .semantic_digest()
                            .clone(),
                    },
                )
                .collect(),
            })
            .expect("provider bindings");
        let variables = variable_contract();

        WorkflowRevisionSemanticContracts::restore(
            &provider_workflow,
            provider_bindings.canonical_acl(),
            provider_bindings.digest().as_str(),
            provider_registry.canonical_acl(),
            provider_registry.digest().as_str(),
            variables.canonical_acl(),
            variables.digest().as_str(),
        )
        .expect("historic provider descriptor snapshot remains readable");

        let contracts = WorkflowRevisionSemanticContracts::create(
            &provider_workflow,
            provider_bindings,
            provider_registry,
            variables,
        )
        .expect("deferred internal composition remains structurally valid");
        let error = contracts
            .validate_runtime_dispatch_support(&provider_workflow)
            .expect_err("unwired user-authored dispatch must remain unavailable");
        assert!(
            error.contains("has no admitted Cloud runtime dispatch port"),
            "unexpected {kind:?} admission error: {error}"
        );
    }
}

pub(super) fn composite_workflow() -> WorkflowSpec {
    let mut iteration = step("iteration", WorkflowStepKind::Subworkflow);
    iteration.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Workflow,
        capability_type: CapabilityType::WorkflowRevision,
        resource_id: WorkflowDefinitionId::new().as_uuid(),
        revision: WorkflowRevisionId::new().to_string(),
        digest: digest('9'),
        capability: "workflow.run".into(),
    });
    WorkflowSpec {
        name: "Composite workflow".into(),
        description: String::new(),
        steps: vec![
            step("input", WorkflowStepKind::Input),
            iteration,
            step("output", WorkflowStepKind::Output),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-iteration".into(),
                source: "input".into(),
                target: "iteration".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "iteration-output".into(),
                source: "iteration".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    }
}

pub(super) fn composite_registry() -> WorkflowStepDescriptorRegistry {
    let mut iteration = descriptor(
        "workflow.iteration",
        WorkflowStepKind::Subworkflow,
        "items",
        "result",
    );
    iteration.semantic_profile = "workflow.iteration".into();
    iteration.execution_class = WorkflowStepExecutionClass::CompositeRegion;
    iteration.required_bindings = vec![WorkflowStepBindingKind::CapabilityReference];
    iteration.allowed_capability_types = vec![CapabilityType::WorkflowRevision];
    WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            iteration,
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
            ),
        ],
    })
    .expect("composite registry")
}

pub(super) fn composite_bindings(
    registry: &WorkflowStepDescriptorRegistry,
) -> WorkflowStepDescriptorBindings {
    WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: [
            ("input", "workflow.input"),
            ("iteration", "workflow.iteration"),
            ("output", "workflow.output"),
        ]
        .into_iter()
        .map(|(step_id, descriptor_id)| WorkflowStepDescriptorBinding {
            step_id: step_id.into(),
            descriptor_id: descriptor_id.into(),
            descriptor_revision: "1.0.0".into(),
            semantic_digest: registry
                .resolve(descriptor_id, "1.0.0")
                .expect("descriptor")
                .semantic_digest()
                .clone(),
        })
        .collect(),
    })
    .expect("composite bindings")
}

pub(super) fn composite_regions() -> WorkflowCompositeRegions {
    WorkflowCompositeRegions::from_spec(WorkflowCompositeRegionsSpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        regions: vec![WorkflowCompositeRegionPolicy::Iteration(
            WorkflowIterationRegionPolicy {
                step_id: "iteration".into(),
                maximum_items: 1_000,
                maximum_concurrency: 10,
                failure_mode: WorkflowIterationFailureMode::Terminate,
            },
        )],
    })
    .expect("composite regions")
}

#[test]
fn semantic_contracts_require_exact_composite_region_material_for_new_publication() {
    let workflow = composite_workflow();
    let registry = composite_registry();
    let bindings = composite_bindings(&registry);
    let variables = variable_contract();

    let error = WorkflowRevisionSemanticContracts::create(
        &workflow,
        bindings.clone(),
        registry.clone(),
        variables.clone(),
    )
    .expect_err("new composite publication requires region material");
    assert!(error.contains("require immutable region material"));

    let legacy = WorkflowRevisionSemanticContracts::restore(
        &workflow,
        bindings.canonical_acl(),
        bindings.digest().as_str(),
        registry.canonical_acl(),
        registry.digest().as_str(),
        variables.canonical_acl(),
        variables.digest().as_str(),
    )
    .expect("pre-contract composite revision remains readable");
    assert!(legacy.composite_regions().is_none());

    let regions = composite_regions();
    let contracts = WorkflowRevisionSemanticContracts::create_with_optional_contracts(
        &workflow,
        bindings,
        registry,
        variables,
        None,
        Some(regions.clone()),
    )
    .expect("composite semantic contracts");
    assert_eq!(contracts.composite_regions(), Some(&regions));
    assert_eq!(contracts.persisted_contracts().len(), 4);
    assert!(contracts.persisted_contracts().iter().any(|contract| {
        contract.kind == WorkflowRevisionSemanticContractKind::CompositeRegions
    }));
    assert_ne!(contracts.digest(), legacy.digest());
}

#[test]
fn semantic_contracts_reject_composite_profile_or_child_revision_drift() {
    let workflow = composite_workflow();
    let registry = composite_registry();
    let bindings = composite_bindings(&registry);
    let variables = variable_contract();
    let mut regions_spec = composite_regions().spec().clone();
    regions_spec.regions = vec![WorkflowCompositeRegionPolicy::Loop(
        WorkflowLoopRegionPolicy {
            step_id: "iteration".into(),
            maximum_iterations: 10,
            time_budget_seconds: 60,
            termination_path: vec!["done".into()],
        },
    )];
    let regions = WorkflowCompositeRegions::from_spec(regions_spec).expect("loop policy");
    assert!(
        WorkflowRevisionSemanticContracts::create_with_optional_contracts(
            &workflow,
            bindings.clone(),
            registry.clone(),
            variables.clone(),
            None,
            Some(regions),
        )
        .is_err()
    );

    let mut nil_workflow = workflow.clone();
    nil_workflow.steps[1]
        .capability
        .as_mut()
        .expect("child capability")
        .revision = uuid::Uuid::nil().to_string();
    assert!(
        WorkflowRevisionSemanticContracts::create_with_optional_contracts(
            &nil_workflow,
            bindings.clone(),
            registry.clone(),
            variables.clone(),
            None,
            Some(composite_regions()),
        )
        .is_err()
    );

    let mut drifted_workflow = workflow;
    drifted_workflow.steps[1]
        .capability
        .as_mut()
        .expect("child capability")
        .revision = "latest".into();
    assert!(
        WorkflowRevisionSemanticContracts::create_with_optional_contracts(
            &drifted_workflow,
            bindings,
            registry,
            variables,
            None,
            Some(composite_regions()),
        )
        .is_err()
    );
}

#[test]
fn semantic_contracts_bind_digest_only_defaults_to_exact_immutable_material() {
    let default = WorkflowVariableDefault::new(
        "fallback",
        json!({"priority": "normal", "source": "revision-default"}),
    )
    .expect("default");
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![WorkflowVariableDeclaration {
            name: "fallback".into(),
            scope: WorkflowVariableScope::Run,
            value_type: WorkflowDataType::Object,
            value_schema_digest: digest('a'),
            source_schema_digest: None,
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Deterministic,
            required: false,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: None,
            default_value_digest: Some(default.digest.clone()),
        }],
        reads: vec![WorkflowVariableRead {
            id: "output-fallback".into(),
            variable: "fallback".into(),
            consumer_step_id: "output".into(),
            consumer_region_id: None,
            target_port: "result".into(),
            path: Vec::new(),
            expected_type: WorkflowDataType::Object,
            expected_schema_digest: digest('a'),
            required: true,
            mode: WorkflowVariableReadMode::DirectValue,
        }],
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .expect("variables");
    let defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: variables.id().into(),
        revision: variables.revision().into(),
        values: vec![default],
    })
    .expect("defaults");
    let registry = registry();

    let missing = WorkflowRevisionSemanticContracts::create(
        &workflow(),
        bindings(&registry),
        registry.clone(),
        variables.clone(),
    )
    .expect_err("digest-only declarations require material");
    assert!(missing.contains("without immutable material"));

    let legacy_bindings = bindings(&registry);
    let restored_legacy = WorkflowRevisionSemanticContracts::restore(
        &workflow(),
        legacy_bindings.canonical_acl(),
        legacy_bindings.digest().as_str(),
        registry.canonical_acl(),
        registry.digest().as_str(),
        variables.canonical_acl(),
        variables.digest().as_str(),
    )
    .expect("pre-107 digest-only revision remains readable");
    assert!(restored_legacy.variable_defaults().is_none());

    let contracts = WorkflowRevisionSemanticContracts::create_with_defaults(
        &workflow(),
        bindings(&registry),
        registry,
        variables,
        Some(defaults.clone()),
    )
    .expect("semantic contracts with defaults");
    assert_eq!(contracts.variable_defaults(), Some(&defaults));
    assert_eq!(contracts.persisted_contracts().len(), 4);
    assert_eq!(
        contracts.persisted_contracts()[3].kind,
        WorkflowRevisionSemanticContractKind::VariableDefaults
    );
}

#[test]
fn semantic_contracts_bind_every_step_and_exclude_presentation_from_the_digest() {
    let first_registry = registry();
    let first = WorkflowRevisionSemanticContracts::create(
        &workflow(),
        bindings(&first_registry),
        first_registry,
        variable_contract(),
    )
    .expect("semantic contracts");

    let mut changed = registry().descriptors()[0].spec().presentation.clone();
    changed.label = "Invocation".into();
    let mut registry_spec = WorkflowStepDescriptorRegistrySpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
            ),
        ],
    };
    registry_spec.descriptors[0].presentation = changed;
    let second_registry =
        WorkflowStepDescriptorRegistry::from_spec(registry_spec).expect("registry");
    let second = WorkflowRevisionSemanticContracts::create(
        &workflow(),
        bindings(&second_registry),
        second_registry,
        variable_contract(),
    )
    .expect("semantic contracts");
    assert_eq!(first.digest(), second.digest());
    assert_ne!(
        first.descriptor_registry().digest(),
        second.descriptor_registry().digest()
    );
}

#[test]
fn semantic_contracts_fail_closed_on_missing_or_drifted_bindings() {
    let registry = registry();
    let missing = bindings(&registry).clone();
    let mut missing_spec = WorkflowStepDescriptorBindingsSpec {
        id: missing.id().into(),
        revision: missing.revision().into(),
        compiler_schema_version: missing.compiler_schema_version(),
        bindings: missing.bindings().to_vec(),
    };
    missing_spec.bindings.pop();
    assert!(WorkflowStepDescriptorBindings::from_spec(missing_spec).is_err());

    let mut drifted_spec = WorkflowStepDescriptorBindingsSpec {
        id: missing.id().into(),
        revision: missing.revision().into(),
        compiler_schema_version: missing.compiler_schema_version(),
        bindings: missing.bindings().to_vec(),
    };
    drifted_spec.bindings[0].semantic_digest = digest('f');
    let drifted = WorkflowStepDescriptorBindings::from_spec(drifted_spec).expect("bindings");
    assert!(WorkflowRevisionSemanticContracts::create(
        &workflow(),
        drifted,
        registry,
        variable_contract(),
    )
    .is_err());
}

#[test]
fn semantic_contracts_reject_bindings_the_v2_compiler_cannot_prove() {
    for unsupported in [
        WorkflowStepBindingKind::ReleaseReference,
        WorkflowStepBindingKind::SecretReference,
        WorkflowStepBindingKind::EgressPolicy,
    ] {
        let mut registry_spec = WorkflowStepDescriptorRegistrySpec {
            id: "support.bound".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            descriptors: vec![
                descriptor(
                    "workflow.input",
                    WorkflowStepKind::Input,
                    "invocation",
                    "value",
                ),
                descriptor(
                    "workflow.output",
                    WorkflowStepKind::Output,
                    "result",
                    "value",
                ),
            ],
        };
        registry_spec.descriptors[1].required_bindings = vec![unsupported];
        let registry = WorkflowStepDescriptorRegistry::from_spec(registry_spec).expect("registry");
        let error = WorkflowRevisionSemanticContracts::create(
            &workflow(),
            bindings(&registry),
            registry,
            variable_contract(),
        )
        .expect_err("unsupported binding must fail closed");
        assert!(
            error.contains(unsupported.as_str()),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn semantic_contracts_validate_typed_reads_against_descriptor_inputs() {
    let registry = registry();
    let mut variables = variable_contract().spec().clone();
    variables.reads[0].target_port = "missing".into();
    let error = WorkflowRevisionSemanticContracts::create(
        &workflow(),
        bindings(&registry),
        registry,
        WorkflowVariableContract::from_spec(variables).expect("variables"),
    )
    .expect_err("missing descriptor port");
    assert!(error.contains("undeclared descriptor input"));

    let mut registry_spec = WorkflowStepDescriptorRegistrySpec {
        id: "support.bound".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
            ),
        ],
    };
    registry_spec.descriptors[1].input_ports[0].value_type = WorkflowDataType::String;
    let registry = WorkflowStepDescriptorRegistry::from_spec(registry_spec).expect("registry");
    let error = WorkflowRevisionSemanticContracts::create(
        &workflow(),
        bindings(&registry),
        registry,
        variable_contract(),
    )
    .expect_err("mismatched descriptor port type");
    assert!(error.contains("type does not match descriptor input"));
}

#[test]
fn plan_v1_serialization_does_not_gain_v2_contract_fields() {
    let input = crate::modules::workflow::test_support::workflow_run_input().expect("run input");
    let encoded = serde_json::to_value(&input.plan).expect("plan JSON");
    let plan = encoded.as_object().expect("plan object");
    assert!(!plan.contains_key("semantic_contract_set_digest"));
    assert!(!plan.contains_key("variable_contract_digest"));
    assert!(!plan.contains_key("composite_regions_digest"));
    assert!(encoded["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .all(|step| !step.as_object().expect("step").contains_key("descriptor")));
}

#[test]
fn semantic_revision_lineage_cannot_downgrade_to_legacy_authority() {
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Object,
            fields: Vec::new(),
        }))
        .expect("data schema");
    let input_configuration = WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
        WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
    ))
    .expect("input configuration");
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))
        .expect("output configuration");
    let bound_workflow = WorkflowSpec {
        steps: vec![
            WorkflowStepSpec {
                configuration_digest: input_configuration.digest().clone(),
                input_schema_digest: data_schema.digest().clone(),
                output_schema_digest: data_schema.digest().clone(),
                ..step("input", WorkflowStepKind::Input)
            },
            WorkflowStepSpec {
                configuration_digest: output_configuration.digest().clone(),
                input_schema_digest: data_schema.digest().clone(),
                output_schema_digest: data_schema.digest().clone(),
                ..step("output", WorkflowStepKind::Output)
            },
        ],
        ..workflow()
    };
    let contract = WorkflowContract::from_spec(bound_workflow.clone()).expect("workflow");
    let registry = registry();
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        declarations: vec![WorkflowVariableDeclaration {
            value_schema_digest: data_schema.digest().clone(),
            source_schema_digest: Some(data_schema.digest().clone()),
            ..variable_contract().spec().declarations[0].clone()
        }],
        reads: vec![WorkflowVariableRead {
            expected_schema_digest: data_schema.digest().clone(),
            ..variable_contract().spec().reads[0].clone()
        }],
        ..variable_contract().spec().clone()
    })
    .expect("variables");
    let parent = WorkflowRevision::initial_with_semantic_contracts(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowDefinitionId::new(),
        WorkflowRevisionId::new(),
        contract.clone(),
        vec![
            data_schema.clone(),
            input_configuration.clone(),
            output_configuration.clone(),
        ],
        WorkflowRevisionSemanticContracts::create(
            &bound_workflow,
            bindings(&registry),
            registry,
            variables,
        )
        .expect("semantic contracts"),
        PrincipalId::new(),
        Utc::now(),
    )
    .expect("parent");
    let error = WorkflowRevision::successor(
        &parent,
        WorkflowRevisionId::new(),
        contract,
        vec![data_schema, input_configuration, output_configuration],
        PrincipalId::new(),
        Utc::now(),
    )
    .expect_err("semantic downgrade");
    assert!(error.contains("cannot remove"));
}

#[path = "workflow_revision_semantic_contracts_application_tests.rs"]
mod application_tests;

mod execution_fallback;
#[path = "workflow_revision_semantic_contracts_tests/list_operator.rs"]
mod list_operator;
#[path = "workflow_revision_semantic_contracts_tests/variable_aggregate.rs"]
mod variable_aggregate;
