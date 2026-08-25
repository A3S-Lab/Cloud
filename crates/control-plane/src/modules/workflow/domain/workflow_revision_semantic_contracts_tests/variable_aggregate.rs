use super::*;

struct VariableAggregateFixture {
    workflow: WorkflowSpec,
    payloads: Vec<WorkflowPayload>,
    semantic_contracts: WorkflowRevisionSemanticContracts,
}

fn configuration(kind: WorkflowStepKind, template: Option<&str>) -> WorkflowPayload {
    let mut value = WorkflowStepConfiguration::empty(kind);
    value.template = template.map(str::to_owned);
    WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(value))
        .expect("configuration")
}

fn data_schema(value_type: WorkflowDataType, fields: Vec<WorkflowDataField>) -> WorkflowPayload {
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

fn typed_port(name: &str, value_type: WorkflowDataType, required: bool) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type,
        cardinality: WorkflowStepPortCardinality::Single,
        required,
        dynamic: false,
    }
}

fn fixture(descriptor_id: &str, candidate_port_required: bool) -> VariableAggregateFixture {
    let object_schema = data_schema(WorkflowDataType::Object, Vec::new());
    let string_schema = data_schema(WorkflowDataType::String, Vec::new());
    let aggregate_input_schema = data_schema(
        WorkflowDataType::Object,
        vec![
            field("left", WorkflowDataType::String, false),
            field("right", WorkflowDataType::String, false),
        ],
    );
    let aggregate_output_schema = data_schema(
        WorkflowDataType::Object,
        vec![field("output", WorkflowDataType::String, true)],
    );
    let final_output_schema = data_schema(
        WorkflowDataType::Object,
        vec![field("result", WorkflowDataType::String, true)],
    );
    let input_configuration = configuration(WorkflowStepKind::Input, None);
    let template_configuration = configuration(WorkflowStepKind::Transform, Some("value"));
    let output_configuration = configuration(WorkflowStepKind::Output, None);
    let mut aggregate_configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    aggregate_configuration.local_transform =
        Some(WorkflowLocalTransformConfiguration::VariableAggregate(
            WorkflowVariableAggregateConfiguration {
                group_enabled: false,
                groups: vec![WorkflowVariableAggregateGroup {
                    output_port: "output".into(),
                    output_type: WorkflowDataType::String,
                    candidates: vec![
                        WorkflowVariableAggregateCandidate {
                            input_port: "left".into(),
                            ordinal: 0,
                        },
                        WorkflowVariableAggregateCandidate {
                            input_port: "right".into(),
                            ordinal: 1,
                        },
                    ],
                }],
            },
        ));
    let aggregate_configuration = WorkflowPayload::from_content(
        WorkflowPayloadContent::Configuration(aggregate_configuration),
    )
    .expect("Variable Aggregator configuration");

    let workflow_step =
        |id: &str,
         kind: WorkflowStepKind,
         configuration: &WorkflowPayload,
         input_schema: &WorkflowPayload,
         output_schema: &WorkflowPayload| WorkflowStepSpec {
            id: id.into(),
            label: id.into(),
            kind,
            configuration_digest: configuration.digest().clone(),
            input_schema_digest: input_schema.digest().clone(),
            output_schema_digest: output_schema.digest().clone(),
            policy_digest: None,
            capability: None,
        };
    let workflow = WorkflowSpec {
        name: "Variable Aggregator".into(),
        description: String::new(),
        steps: vec![
            workflow_step(
                "input",
                WorkflowStepKind::Input,
                &input_configuration,
                &object_schema,
                &object_schema,
            ),
            workflow_step(
                "left",
                WorkflowStepKind::Transform,
                &template_configuration,
                &object_schema,
                &string_schema,
            ),
            workflow_step(
                "right",
                WorkflowStepKind::Transform,
                &template_configuration,
                &object_schema,
                &string_schema,
            ),
            workflow_step(
                "aggregate",
                WorkflowStepKind::Transform,
                &aggregate_configuration,
                &aggregate_input_schema,
                &aggregate_output_schema,
            ),
            workflow_step(
                "output",
                WorkflowStepKind::Output,
                &output_configuration,
                &final_output_schema,
                &final_output_schema,
            ),
        ],
        edges: vec![
            edge("input-left", "input", "left"),
            edge("input-right", "input", "right"),
            edge("left-aggregate", "left", "aggregate"),
            edge("right-aggregate", "right", "aggregate"),
            edge("aggregate-output", "aggregate", "output"),
        ],
    };

    let mut input_descriptor = descriptor(
        "workflow.input",
        WorkflowStepKind::Input,
        "invocation",
        "value",
    );
    input_descriptor.input_ports = Vec::new();
    input_descriptor.output_ports = vec![typed_port("value", WorkflowDataType::Object, true)];
    let mut template_descriptor = descriptor(
        "workflow.template",
        WorkflowStepKind::Transform,
        "input",
        "output",
    );
    template_descriptor.input_ports = Vec::new();
    template_descriptor.output_ports = vec![typed_port("output", WorkflowDataType::String, true)];
    let mut aggregate_descriptor =
        descriptor(descriptor_id, WorkflowStepKind::Transform, "left", "output");
    aggregate_descriptor.semantic_profile = descriptor_id.into();
    aggregate_descriptor.input_ports = vec![
        typed_port("left", WorkflowDataType::String, candidate_port_required),
        typed_port("right", WorkflowDataType::String, false),
    ];
    aggregate_descriptor.output_ports = vec![typed_port("output", WorkflowDataType::String, true)];
    let mut output_descriptor = descriptor(
        "workflow.output",
        WorkflowStepKind::Output,
        "result",
        "value",
    );
    output_descriptor.input_ports = vec![typed_port("result", WorkflowDataType::String, true)];
    output_descriptor.output_ports = vec![typed_port("value", WorkflowDataType::Object, true)];
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.variable-aggregate".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            input_descriptor,
            template_descriptor,
            aggregate_descriptor,
            output_descriptor,
        ],
    })
    .expect("Variable Aggregator registry");
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "support.variable-aggregate".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: [
            ("input", "workflow.input"),
            ("left", "workflow.template"),
            ("right", "workflow.template"),
            ("aggregate", descriptor_id),
            ("output", "workflow.output"),
        ]
        .into_iter()
        .map(|(step_id, descriptor_id)| WorkflowStepDescriptorBinding {
            step_id: step_id.into(),
            descriptor_id: descriptor_id.into(),
            descriptor_revision: "1.0.0".into(),
            semantic_digest: registry
                .resolve(descriptor_id, "1.0.0")
                .expect("bound descriptor")
                .semantic_digest()
                .clone(),
        })
        .collect(),
    })
    .expect("Variable Aggregator bindings");
    let declaration = |name: &str,
                       source_step_id: &str,
                       source_schema_digest: &Sha256Digest,
                       source_path: Vec<String>,
                       required| WorkflowVariableDeclaration {
        name: name.into(),
        scope: WorkflowVariableScope::NodeOutput,
        value_type: WorkflowDataType::String,
        value_schema_digest: string_schema.digest().clone(),
        source_schema_digest: Some(source_schema_digest.clone()),
        storage_class: WorkflowVariableStorageClass::Inline,
        mutation_mode: WorkflowVariableMutationMode::Immutable,
        required,
        source_step_id: Some(source_step_id.into()),
        source_path,
        region_id: None,
        default_value_digest: None,
    };
    let read = |id: &str, variable: &str, consumer: &str, target_port: &str, required| {
        WorkflowVariableRead {
            id: id.into(),
            variable: variable.into(),
            consumer_step_id: consumer.into(),
            consumer_region_id: None,
            target_port: target_port.into(),
            path: Vec::new(),
            expected_type: WorkflowDataType::String,
            expected_schema_digest: string_schema.digest().clone(),
            required,
            mode: WorkflowVariableReadMode::DirectValue,
        }
    };
    let variable_contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.variable-aggregate".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![
            declaration(
                "left_value",
                "left",
                string_schema.digest(),
                Vec::new(),
                false,
            ),
            declaration(
                "right_value",
                "right",
                string_schema.digest(),
                Vec::new(),
                false,
            ),
            declaration(
                "aggregate_value",
                "aggregate",
                aggregate_output_schema.digest(),
                vec!["output".into()],
                true,
            ),
        ],
        reads: vec![
            read("aggregate-left", "left_value", "aggregate", "left", false),
            read(
                "aggregate-right",
                "right_value",
                "aggregate",
                "right",
                false,
            ),
            read("output-result", "aggregate_value", "output", "result", true),
        ],
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .expect("Variable Aggregator variables");
    let semantic_contracts =
        WorkflowRevisionSemanticContracts::create(&workflow, bindings, registry, variable_contract)
            .expect("Variable Aggregator semantic contracts");

    VariableAggregateFixture {
        workflow,
        payloads: vec![
            object_schema,
            string_schema,
            aggregate_input_schema,
            aggregate_output_schema,
            final_output_schema,
            input_configuration,
            template_configuration,
            aggregate_configuration,
            output_configuration,
        ],
        semantic_contracts,
    }
}

fn edge(id: &str, source: &str, target: &str) -> WorkflowEdgeSpec {
    WorkflowEdgeSpec {
        id: id.into(),
        source: source.into(),
        target: target.into(),
        source_handle: None,
    }
}

fn publish(fixture: VariableAggregateFixture) -> Result<WorkflowRevision, String> {
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
fn publication_requires_the_exact_descriptor_and_optional_candidate_ports() {
    publish(fixture("workflow.variable-aggregate", false))
        .expect("exact Variable Aggregator publication");

    let descriptor_error = publish(fixture("workflow.custom-aggregate", false))
        .expect_err("custom Transform descriptor must not self-admit aggregation");
    assert!(descriptor_error.contains("exact Workflow-owned workflow.variable-aggregate"));

    let required_error = publish(fixture("workflow.variable-aggregate", true))
        .expect_err("candidate descriptor ports must remain optional");
    assert!(required_error.contains("optional, static, single, and type-exact"));

    let mut missing_configuration = fixture("workflow.variable-aggregate", false);
    let template_configuration_digest = missing_configuration
        .workflow
        .steps
        .iter()
        .find(|step| step.id == "left")
        .expect("template step")
        .configuration_digest
        .clone();
    missing_configuration
        .workflow
        .steps
        .iter_mut()
        .find(|step| step.id == "aggregate")
        .expect("aggregate step")
        .configuration_digest = template_configuration_digest;
    missing_configuration
        .payloads
        .retain(|payload| payload.schema() != WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA);
    let missing_error = publish(missing_configuration)
        .expect_err("the reserved descriptor must retain its exact configuration");
    assert!(missing_error.contains("lost its exact configuration"));
}

#[test]
fn compiler_pins_variable_aggregation_to_run_v20_without_a_new_plan_schema() {
    let fixture = fixture("workflow.variable-aggregate", false);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let principal_id = PrincipalId::new();
    let now = Utc::now();
    let contract = WorkflowContract::from_spec(fixture.workflow).expect("Workflow contract");
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
    .expect("Workflow revision");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        "Variable Aggregator".into(),
        String::new(),
        revision_id,
        contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("Workflow definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Variable Aggregator ontology".into(),
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
    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "Aggregate candidates".into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: revision_id,
        workflow_digest: contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: None,
        input: json!({}),
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
    .expect("compiled Variable Aggregator run");
    assert_eq!(
        run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V20
    );
    assert_eq!(
        run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20
    );
    assert_eq!(
        run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V20
    );
    run.run.execution_input.validate().expect("valid v20 input");

    let mut downgraded = run.run.execution_input;
    downgraded.schema = WORKFLOW_RUN_INPUT_SCHEMA_V2.into();
    downgraded.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2.into();
    downgraded.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V2.into();
    assert!(downgraded
        .validate()
        .expect_err("Variable Aggregator cannot downgrade to v2")
        .contains("runtime generation v20 or a composing v21/v22 generation"));
}
