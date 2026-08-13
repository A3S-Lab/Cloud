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

fn variable_contract() -> WorkflowVariableContract {
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

#[test]
fn compiler_emits_plan_v2_and_pins_its_variable_contract_into_a_v2_run() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let principal_id = PrincipalId::new();
    let now = Utc::now();

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
        name: "Bound workflow".into(),
        description: String::new(),
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
        edges: workflow().edges,
    };
    let contract = WorkflowContract::from_spec(bound_workflow.clone()).expect("workflow");
    let registry = registry_with_output_bindings(vec![WorkflowStepBindingKind::PlacementPolicy]);
    let semantic_contracts = WorkflowRevisionSemanticContracts::create(
        &bound_workflow,
        bindings(&registry),
        registry,
        WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
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
        .expect("variables"),
    )
    .expect("semantic contracts");
    let revision = WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        contract.clone(),
        vec![data_schema, input_configuration, output_configuration],
        semantic_contracts,
        principal_id,
        now,
    )
    .expect("revision");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        bound_workflow.name,
        bound_workflow.description,
        revision_id,
        contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Bound ontology".into(),
        description: String::new(),
        object_types: vec![OntologyObjectType {
            id: "request".into(),
            label: "Request".into(),
            schema_digest: digest('a'),
            key_fields: vec!["id".into()],
        }],
        relation_types: Vec::new(),
        rules: Vec::new(),
    })
    .expect("ontology");
    let ontology_revision = OntologyRevision::initial(
        organization_id,
        project_id,
        ontology_id,
        ontology_revision_id,
        ontology_contract.clone(),
        principal_id,
        now,
    );
    let goal_spec = WorkflowGoalSpec {
        name: "Bound goal".into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: revision_id,
        workflow_digest: contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: None,
        input: json!({}),
    };
    let missing_environment = WorkflowGoalContract::from_spec(goal_spec.clone()).expect("goal");
    let error = WorkflowPlanCompiler::compile_goal(
        WorkflowGoalId::new(),
        PlanRevisionId::new(),
        missing_environment,
        &definition,
        &revision,
        &ontology_revision,
        principal_id,
        now,
    )
    .expect_err("placement without environment");
    assert!(
        error.contains("placement-policy"),
        "unexpected error: {error}"
    );
    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        environment_id: Some(EnvironmentId::new()),
        ..goal_spec
    })
    .expect("goal");
    let compiled = WorkflowPlanCompiler::compile_goal(
        WorkflowGoalId::new(),
        PlanRevisionId::new(),
        goal_contract,
        &definition,
        &revision,
        &ontology_revision,
        principal_id,
        now,
    )
    .expect("compiled");
    assert_eq!(compiled.plan_revision.plan.schema, WORKFLOW_PLAN_SCHEMA_V2);
    assert_eq!(
        compiled.plan_revision.plan.compiler_revision,
        WORKFLOW_PLAN_COMPILER_REVISION_V2
    );
    assert_eq!(
        compiled
            .plan_revision
            .plan
            .variable_contract_digest
            .as_ref(),
        Some(
            revision
                .semantic_contracts
                .as_ref()
                .expect("semantics")
                .variable_contract()
                .digest()
        )
    );
    assert!(compiled
        .plan_revision
        .plan
        .steps
        .iter()
        .all(|step| step.descriptor.is_some()));
    let compiled_run = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("compiled v2 run");
    assert_eq!(
        compiled_run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V2
    );
    assert_eq!(
        compiled_run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2
    );
    assert_eq!(
        compiled_run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V2
    );
    let resolved_variables = compiled_run
        .run
        .execution_input
        .variable_contract
        .as_ref()
        .expect("resolved variable contract");
    let variables = revision
        .semantic_contracts
        .as_ref()
        .expect("semantic contracts")
        .variable_contract();
    assert_eq!(resolved_variables.canonical_acl, variables.canonical_acl());
    assert_eq!(&resolved_variables.digest, variables.digest());
    compiled_run
        .run
        .execution_input
        .validate()
        .expect("valid v2 run input");
}
