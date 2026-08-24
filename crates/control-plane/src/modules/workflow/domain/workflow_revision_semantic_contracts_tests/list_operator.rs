use super::*;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    Sha256Digest, WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use chrono::Utc;
use serde_json::json;

struct Fixture {
    workflow: WorkflowSpec,
    payloads: Vec<WorkflowPayload>,
    semantic_contracts: WorkflowRevisionSemanticContracts,
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn schema(value_type: WorkflowDataType, fields: Vec<WorkflowDataField>) -> WorkflowPayload {
    WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
        value_type,
        fields,
    }))
    .expect("data schema")
}

fn field(name: &str, value_type: WorkflowDataType, required: bool) -> WorkflowDataField {
    WorkflowDataField {
        name: name.into(),
        value_type,
        required,
    }
}

fn port(name: &str, value_type: WorkflowDataType, required: bool) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type,
        cardinality: WorkflowStepPortCardinality::Single,
        required,
        dynamic: false,
    }
}

fn descriptor(
    id: &str,
    kind: WorkflowStepKind,
    input_ports: Vec<WorkflowStepPort>,
    output_ports: Vec<WorkflowStepPort>,
) -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: id.into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(kind),
        semantic_profile: id.into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports,
        output_ports,
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

fn fixture(descriptor_id: &str, operation_input_required: bool) -> Fixture {
    let input_schema = schema(
        WorkflowDataType::Object,
        vec![
            field("items", WorkflowDataType::Array, true),
            field("minimum", WorkflowDataType::Number, true),
        ],
    );
    let array_schema = schema(WorkflowDataType::Array, Vec::new());
    let number_schema = schema(WorkflowDataType::Number, Vec::new());
    let operator_input_schema = schema(
        WorkflowDataType::Object,
        vec![
            field("items", WorkflowDataType::Array, true),
            field(
                "minimum",
                WorkflowDataType::Number,
                operation_input_required,
            ),
        ],
    );
    let operator_output_schema = schema(
        WorkflowDataType::Object,
        vec![
            field("first_record", WorkflowDataType::Number, false),
            field("last_record", WorkflowDataType::Number, false),
            field("result", WorkflowDataType::Array, true),
        ],
    );
    let final_output_schema = schema(
        WorkflowDataType::Object,
        vec![field("result", WorkflowDataType::Array, true)],
    );

    let input_configuration = WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
        WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
    ))
    .expect("input configuration");
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))
        .expect("output configuration");
    let mut configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    configuration.local_transform = Some(WorkflowLocalTransformConfiguration::ListOperator(
        WorkflowListOperatorConfiguration {
            source_port: "items".into(),
            item_type: WorkflowDataType::Number,
            conditions: vec![WorkflowListOperatorFilterCondition {
                id: "minimum".into(),
                ordinal: 0,
                key: None,
                value_type: WorkflowDataType::Number,
                operator: WorkflowListOperatorFilterOperator::GreaterThanOrEqual,
                operand: Some(WorkflowListOperatorOperand::InputPort {
                    input_port: "minimum".into(),
                    value_type: WorkflowDataType::Number,
                }),
            }],
            extract: None,
            order: Some(WorkflowListOperatorOrder {
                key: None,
                value_type: WorkflowDataType::Number,
                direction: WorkflowListOperatorOrderDirection::Desc,
            }),
            limit: Some(5),
        },
    ));
    let list_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(configuration))
            .expect("List Operator configuration");

    let step = |id: &str,
                kind: WorkflowStepKind,
                configuration: &WorkflowPayload,
                input: &WorkflowPayload,
                output: &WorkflowPayload| WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest: configuration.digest().clone(),
        input_schema_digest: input.digest().clone(),
        output_schema_digest: output.digest().clone(),
        policy_digest: None,
        capability: None,
    };
    let workflow = WorkflowSpec {
        name: "List Operator".into(),
        description: String::new(),
        steps: vec![
            step(
                "input",
                WorkflowStepKind::Input,
                &input_configuration,
                &input_schema,
                &input_schema,
            ),
            step(
                "list",
                WorkflowStepKind::Transform,
                &list_configuration,
                &operator_input_schema,
                &operator_output_schema,
            ),
            step(
                "output",
                WorkflowStepKind::Output,
                &output_configuration,
                &final_output_schema,
                &final_output_schema,
            ),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-list".into(),
                source: "input".into(),
                target: "list".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "list-output".into(),
                source: "list".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };

    let input_descriptor = descriptor(
        "workflow.input",
        WorkflowStepKind::Input,
        Vec::new(),
        vec![port("value", WorkflowDataType::Object, true)],
    );
    let mut list_descriptor = descriptor(
        descriptor_id,
        WorkflowStepKind::Transform,
        vec![
            port("items", WorkflowDataType::Array, true),
            port(
                "minimum",
                WorkflowDataType::Number,
                operation_input_required,
            ),
        ],
        vec![
            port("result", WorkflowDataType::Array, true),
            port("first_record", WorkflowDataType::Number, false),
            port("last_record", WorkflowDataType::Number, false),
        ],
    );
    list_descriptor.semantic_profile = descriptor_id.into();
    let output_descriptor = descriptor(
        "workflow.output",
        WorkflowStepKind::Output,
        vec![port("result", WorkflowDataType::Array, true)],
        vec![port("value", WorkflowDataType::Object, true)],
    );
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.list-operator".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![input_descriptor, list_descriptor, output_descriptor],
    })
    .expect("registry");
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "support.list-operator".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: [
            ("input", "workflow.input"),
            ("list", descriptor_id),
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
                    .expect("descriptor")
                    .semantic_digest()
                    .clone(),
            },
        )
        .collect(),
    })
    .expect("bindings");
    let declaration =
        |name: &str,
         value_type: WorkflowDataType,
         value_schema: &WorkflowPayload,
         source_step_id: &str,
         source_schema: &WorkflowPayload,
         source_path: Vec<String>| WorkflowVariableDeclaration {
            name: name.into(),
            scope: WorkflowVariableScope::NodeOutput,
            value_type,
            value_schema_digest: value_schema.digest().clone(),
            source_schema_digest: Some(source_schema.digest().clone()),
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required: true,
            source_step_id: Some(source_step_id.into()),
            source_path,
            region_id: None,
            default_value_digest: None,
        };
    let read = |id: &str,
                variable: &str,
                consumer_step_id: &str,
                target_port: &str,
                expected_type: WorkflowDataType,
                value_schema: &WorkflowPayload,
                required: bool| WorkflowVariableRead {
        id: id.into(),
        variable: variable.into(),
        consumer_step_id: consumer_step_id.into(),
        consumer_region_id: None,
        target_port: target_port.into(),
        path: Vec::new(),
        expected_type,
        expected_schema_digest: value_schema.digest().clone(),
        required,
        mode: WorkflowVariableReadMode::DirectValue,
    };
    let variable_contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.list-operator".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![
            declaration(
                "items",
                WorkflowDataType::Array,
                &array_schema,
                "input",
                &input_schema,
                vec!["items".into()],
            ),
            declaration(
                "minimum",
                WorkflowDataType::Number,
                &number_schema,
                "input",
                &input_schema,
                vec!["minimum".into()],
            ),
            declaration(
                "list_result",
                WorkflowDataType::Array,
                &array_schema,
                "list",
                &operator_output_schema,
                vec!["result".into()],
            ),
        ],
        reads: vec![
            read(
                "list-items",
                "items",
                "list",
                "items",
                WorkflowDataType::Array,
                &array_schema,
                true,
            ),
            read(
                "list-minimum",
                "minimum",
                "list",
                "minimum",
                WorkflowDataType::Number,
                &number_schema,
                operation_input_required,
            ),
            read(
                "output-result",
                "list_result",
                "output",
                "result",
                WorkflowDataType::Array,
                &array_schema,
                true,
            ),
        ],
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .expect("variables");
    let semantic_contracts =
        WorkflowRevisionSemanticContracts::create(&workflow, bindings, registry, variable_contract)
            .expect("semantic contracts");
    Fixture {
        workflow,
        payloads: vec![
            input_schema,
            operator_input_schema,
            operator_output_schema,
            final_output_schema,
            input_configuration,
            list_configuration,
            output_configuration,
        ],
        semantic_contracts,
    }
}

fn publish(fixture: Fixture) -> Result<WorkflowRevision, String> {
    let contract = WorkflowContract::from_spec(fixture.workflow)?;
    WorkflowRevision::initial_with_semantic_contracts(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowDefinitionId::new(),
        WorkflowRevisionId::new(),
        contract,
        fixture.payloads,
        fixture.semantic_contracts,
        PrincipalId::new(),
        Utc::now(),
    )
}

#[test]
fn publication_requires_the_exact_descriptor_and_deferred_direct_inputs() {
    publish(fixture("workflow.list-operator", false)).expect("exact publication");
    assert!(publish(fixture("workflow.custom-list", false))
        .expect_err("custom descriptor")
        .contains("exact Workflow-owned workflow.list-operator"));
    assert!(publish(fixture("workflow.list-operator", true))
        .expect_err("eager dynamic operand")
        .contains("operation inputs optional"));

    let mut missing = fixture("workflow.list-operator", false);
    let generic_configuration = WorkflowPayload::from_content(
        WorkflowPayloadContent::Configuration(WorkflowStepConfiguration::empty(
            WorkflowStepKind::Transform,
        )),
    )
    .expect("generic transform configuration");
    missing
        .workflow
        .steps
        .iter_mut()
        .find(|step| step.id == "list")
        .expect("list")
        .configuration_digest = generic_configuration.digest().clone();
    missing
        .payloads
        .retain(|payload| payload.schema() != WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA);
    missing.payloads.push(generic_configuration);
    assert!(publish(missing)
        .expect_err("missing exact configuration")
        .contains("lost its exact configuration"));
}

#[test]
fn compiler_pins_list_operator_to_run_v21_without_a_new_plan_schema() {
    let fixture = fixture("workflow.list-operator", false);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let principal_id = PrincipalId::new();
    let now = Utc::now();
    let contract = WorkflowContract::from_spec(fixture.workflow).expect("contract");
    let revision = WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        contract.clone(),
        fixture.payloads,
        fixture.semantic_contracts,
        principal_id,
        now,
    )
    .expect("revision");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        "List Operator".into(),
        String::new(),
        revision_id,
        contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "List Operator ontology".into(),
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
    .expect("ontology contract");
    let ontology_revision = OntologyRevision::initial(
        organization_id,
        project_id,
        ontology_id,
        ontology_revision_id,
        ontology_contract.clone(),
        principal_id,
        now,
    );
    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "Operate list".into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: revision_id,
        workflow_digest: contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: None,
        input: json!({"items": [1, 3, 2], "minimum": 2}),
    })
    .expect("goal contract");
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
    .expect("compiled goal");
    assert_eq!(compiled.plan_revision.plan.schema, WORKFLOW_PLAN_SCHEMA_V2);
    let run = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("compiled run");
    assert_eq!(
        run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V21
    );
    assert_eq!(
        run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21
    );
    assert_eq!(
        run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V21
    );
    run.run.execution_input.validate().expect("valid v21 input");

    let mut downgraded = run.run.execution_input;
    downgraded.schema = WORKFLOW_RUN_INPUT_SCHEMA_V20.into();
    downgraded.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20.into();
    downgraded.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V20.into();
    assert!(downgraded
        .validate()
        .expect_err("v20 downgrade")
        .contains("exact v21 runtime generation"));
}
