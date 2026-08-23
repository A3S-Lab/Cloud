use super::*;
use crate::modules::shared_kernel::domain::canonical_json_bounded;
use crate::modules::workflow::test_support::application_frame_answer_workflow_run_input;
use std::collections::BTreeMap;

fn frame_authority_for_compiled_plan(
    organization_id: OrganizationId,
    project_id: ProjectId,
    child_plan: &WorkflowPlan,
    child_input: serde_json::Value,
) -> (WorkflowCompositeFrame, WorkflowApplicationFrameAuthority) {
    let (mut parent, _, _) =
        application_frame_answer_workflow_run_input(0).expect("Application frame parent fixture");
    parent.organization_id = organization_id;
    parent.project_id = project_id;
    let capability = parent
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == "iteration")
        .and_then(|step| step.capability.as_mut())
        .expect("Application frame child capability");
    capability.resource_id = child_plan.workflow_definition_id.as_uuid();
    capability.revision = child_plan.workflow_revision_id.to_string();
    capability.digest = child_plan.workflow_digest.clone();
    parent.plan.validate().expect("rewired parent Plan");
    parent.plan_digest = Sha256Digest::from_bytes(
        &canonical_json_bounded(
            &parent.plan,
            WORKFLOW_PLAN_MAX_BYTES,
            "Application frame compiler parent Plan",
        )
        .expect("canonical parent Plan"),
    );
    parent.validate().expect("rewired Application frame parent");
    let variables = parent
        .variable_contract
        .as_ref()
        .expect("parent variable contract")
        .restore()
        .expect("restore parent variables");
    let regions = parent
        .composite_regions
        .as_ref()
        .expect("parent composite regions")
        .restore()
        .expect("restore parent regions");
    let frame = WorkflowCompositeFrame::open(
        WorkflowCompositeFrameRequest {
            organization_id,
            project_id,
            workflow_run_id: parent.workflow_run_id,
            plan_revision_id: parent.plan_revision_id,
            plan_digest: parent.plan_digest.clone(),
            region_step_id: "iteration".into(),
            ordinal: 0,
            effective_input: child_input,
            available_variables: BTreeMap::from([("request".into(), parent.goal_input.clone())]),
        },
        &parent.plan,
        &regions,
        &variables,
        None,
    )
    .expect("exact Application frame");
    let authority = WorkflowApplicationFrameAuthority::from_parent(&parent, &frame)
        .expect("derive Application frame authority")
        .expect("Application frame projection");
    (frame, authority)
}

#[test]
fn compiler_preserves_standalone_v2_and_emits_application_projection_v10() {
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
        bound_workflow.name.clone(),
        bound_workflow.description.clone(),
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

    let application_run = WorkflowRunCompiler::compile_for_application(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("compiled Application run");
    let application_input = &application_run.run.execution_input;
    assert_eq!(application_input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V10);
    assert_eq!(
        application_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10
    );
    assert_eq!(
        application_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V10
    );
    assert_eq!(
        application_input
            .application_projection
            .as_ref()
            .expect("Application projection")
            .final_output_step_id,
        "output"
    );
    application_input.validate().expect("valid v10 run input");
}

#[test]
fn compiler_emits_v11_only_for_exact_application_answer_and_rejects_standalone_dispatch() {
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
        name: "Application Answer workflow".into(),
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
                ..step("answer", WorkflowStepKind::Output)
            },
            WorkflowStepSpec {
                configuration_digest: output_configuration.digest().clone(),
                input_schema_digest: data_schema.digest().clone(),
                output_schema_digest: data_schema.digest().clone(),
                ..step("output", WorkflowStepKind::Output)
            },
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-answer".into(),
                source: "input".into(),
                target: "answer".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "input-output".into(),
                source: "input".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };
    let contract = WorkflowContract::from_spec(bound_workflow.clone()).expect("workflow");
    let mut answer_descriptor = descriptor(
        "application.answer",
        WorkflowStepKind::Output,
        "content",
        "message",
    );
    answer_descriptor.owner = WorkflowStepOwner::Applications;
    answer_descriptor.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
    answer_descriptor.required_bindings = vec![WorkflowStepBindingKind::ReleaseReference];
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.application-answer".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            answer_descriptor,
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
            ),
        ],
    })
    .expect("registry");
    let descriptor_bindings =
        WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
            id: "support.application-answer".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            bindings: [
                ("input", "workflow.input"),
                ("answer", "application.answer"),
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
        .expect("bindings");
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
    let semantic_contracts = WorkflowRevisionSemanticContracts::create(
        &bound_workflow,
        descriptor_bindings,
        registry,
        variables,
    )
    .expect("semantic contracts");
    let classified = semantic_contracts
        .application_output_steps(&bound_workflow)
        .expect("Application outputs");
    assert_eq!(classified.final_output_step_id, "output");
    assert_eq!(classified.answer_step_ids, ["answer".to_owned()].into());
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
        bound_workflow.name.clone(),
        bound_workflow.description.clone(),
        revision_id,
        contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Application Answer ontology".into(),
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
        name: "Application Answer goal".into(),
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
    let standalone_error = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect_err("standalone Answer dispatch");
    assert!(standalone_error.contains("requires Application composition"));
    let application = WorkflowRunCompiler::compile_for_application(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("Application Answer run");
    let input = &application.run.execution_input;
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V11);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V11);
    let projection = input
        .application_projection
        .as_ref()
        .expect("Application projection");
    assert_eq!(projection.final_output_step_id, "output");
    assert_eq!(projection.answer_step_ids, ["answer"]);
    input.validate().expect("valid v11 input");

    let (frame, frame_authority) = frame_authority_for_compiled_plan(
        organization_id,
        project_id,
        &compiled.plan_revision.plan,
        compiled.goal.contract.spec().input.clone(),
    );
    let frame_run = WorkflowRunCompiler::compile_for_application_frame(
        frame.child_workflow_run_id(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
        frame_authority.clone(),
    )
    .expect("Application frame Answer run");
    let frame_input = &frame_run.run.execution_input;
    assert_eq!(frame_input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V13);
    assert_eq!(
        frame_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13
    );
    assert_eq!(
        frame_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V13
    );
    let frame_projection = frame_input
        .application_projection
        .as_ref()
        .expect("Application frame projection");
    assert_eq!(
        frame_projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4
    );
    assert_eq!(frame_projection.answer_step_ids, ["answer"]);
    assert_eq!(
        frame_projection.frame_authority.as_ref(),
        Some(&frame_authority)
    );
    assert!(!frame_projection.projects_application_lifecycle());
    assert!(frame_projection.supports_application_frames());
    frame_input.validate().expect("valid v13 frame input");
}

#[test]
fn compiler_emits_v12_only_for_exact_application_variable_port() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let principal_id = PrincipalId::new();
    let now = Utc::now();
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))
        .expect("data schema");
    let input_configuration = WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
        WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
    ))
    .expect("input configuration");
    let assignment_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Service),
        ))
        .expect("assignment configuration");
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))
        .expect("output configuration");
    let bound_workflow = WorkflowSpec {
        name: "Application variable workflow".into(),
        description: String::new(),
        steps: vec![
            WorkflowStepSpec {
                configuration_digest: input_configuration.digest().clone(),
                input_schema_digest: data_schema.digest().clone(),
                output_schema_digest: data_schema.digest().clone(),
                ..step("input", WorkflowStepKind::Input)
            },
            WorkflowStepSpec {
                configuration_digest: assignment_configuration.digest().clone(),
                input_schema_digest: data_schema.digest().clone(),
                output_schema_digest: data_schema.digest().clone(),
                ..step("assign", WorkflowStepKind::Service)
            },
            WorkflowStepSpec {
                configuration_digest: output_configuration.digest().clone(),
                input_schema_digest: data_schema.digest().clone(),
                output_schema_digest: data_schema.digest().clone(),
                ..step("output", WorkflowStepKind::Output)
            },
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-assign".into(),
                source: "input".into(),
                target: "assign".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "assign-output".into(),
                source: "assign".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };
    let contract = WorkflowContract::from_spec(bound_workflow.clone()).expect("workflow");
    let legacy_error = WorkflowRevision::initial(
        organization_id,
        project_id,
        definition_id,
        WorkflowRevisionId::new(),
        contract.clone(),
        vec![
            data_schema.clone(),
            input_configuration.clone(),
            assignment_configuration.clone(),
            output_configuration.clone(),
        ],
        principal_id,
        now,
    )
    .expect_err("capability-free legacy Service revision");
    assert!(legacy_error.contains("require immutable descriptor semantic contracts"));
    let mut assignment_descriptor = descriptor(
        "application.conversation-variable-assign",
        WorkflowStepKind::Service,
        "input",
        "values",
    );
    assignment_descriptor.owner = WorkflowStepOwner::Applications;
    assignment_descriptor.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
    assignment_descriptor.required_bindings = vec![WorkflowStepBindingKind::ReleaseReference];
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.application-variable".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            assignment_descriptor.clone(),
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
            ),
        ],
    })
    .expect("registry");
    let descriptor_bindings =
        WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
            id: "support.application-variable".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            bindings: [
                ("input", "workflow.input"),
                ("assign", "application.conversation-variable-assign"),
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
        .expect("bindings");
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.application-variable".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![
            WorkflowVariableDeclaration {
                name: "request".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::Any,
                value_schema_digest: data_schema.digest().clone(),
                source_schema_digest: Some(data_schema.digest().clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "conversation_topic".into(),
                scope: WorkflowVariableScope::Application,
                value_type: WorkflowDataType::String,
                value_schema_digest: data_schema.digest().clone(),
                source_schema_digest: None,
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::OptimisticApplicationPort,
                required: false,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "conversation_revision".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::Number,
                value_schema_digest: data_schema.digest().clone(),
                source_schema_digest: Some(data_schema.digest().clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: vec!["conversationRevision".into()],
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "conversation_effect".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::String,
                value_schema_digest: data_schema.digest().clone(),
                source_schema_digest: Some(data_schema.digest().clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: vec!["conversationEffect".into()],
                region_id: None,
                default_value_digest: None,
            },
        ],
        reads: Vec::new(),
        assignments: vec![WorkflowVariableAssignment {
            id: "assign-conversation-topic".into(),
            target_variable: "conversation_topic".into(),
            source_variable: "request".into(),
            writer_step_id: "assign".into(),
            writer_region_id: None,
            source_path: vec!["topic".into()],
            value_type: WorkflowDataType::String,
            value_schema_digest: data_schema.digest().clone(),
            mutation_order: 1,
            expected_revision_variable: Some("conversation_revision".into()),
            idempotency_key_variable: Some("conversation_effect".into()),
        }],
        exports: Vec::new(),
    })
    .expect("variables");
    let semantic_contracts = WorkflowRevisionSemanticContracts::create(
        &bound_workflow,
        descriptor_bindings,
        registry,
        variables,
    )
    .expect("semantic contracts");
    let classified = semantic_contracts
        .application_output_steps(&bound_workflow)
        .expect("Application outputs");
    assert_eq!(classified.final_output_step_id, "output");
    assert_eq!(classified.variable_step_ids, ["assign".to_owned()].into());
    assert_eq!(
        classified.variable_assignment_step_ids,
        ["assign".to_owned()].into()
    );

    let revision = WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        contract.clone(),
        vec![
            data_schema,
            input_configuration,
            assignment_configuration,
            output_configuration,
        ],
        semantic_contracts,
        principal_id,
        now,
    )
    .expect("revision");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        bound_workflow.name.clone(),
        bound_workflow.description.clone(),
        revision_id,
        contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Application variable ontology".into(),
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
        name: "Application variable goal".into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: revision_id,
        workflow_digest: contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: None,
        input: json!({
            "topic": "urgent",
            "conversationRevision": 0,
            "conversationEffect": "effect-1"
        }),
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
    let (frame, frame_authority) = frame_authority_for_compiled_plan(
        organization_id,
        project_id,
        &compiled.plan_revision.plan,
        compiled.goal.contract.spec().input.clone(),
    );
    let frame_error = WorkflowRunCompiler::compile_for_application_frame(
        frame.child_workflow_run_id(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
        frame_authority,
    )
    .expect_err("Application variable access inside a composite frame");
    assert!(frame_error.contains("cannot access Application-scoped variables"));
    let standalone_error = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect_err("standalone Application variable dispatch");
    assert!(standalone_error.contains("requires Application composition"));
    let application = WorkflowRunCompiler::compile_for_application(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("Application variable run");
    let input = &application.run.execution_input;
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V12);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V12);
    let projection = input
        .application_projection
        .as_ref()
        .expect("Application projection");
    assert_eq!(
        projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
    );
    assert_eq!(projection.variable_step_ids, ["assign"]);
    assert_eq!(projection.variable_assignment_step_ids, ["assign"]);
    input.validate().expect("valid v12 input");

    let routed_revision_id = WorkflowRevisionId::new();
    let mut routed_workflow = bound_workflow.clone();
    routed_workflow.edges.push(WorkflowEdgeSpec {
        id: "assign-error-output".into(),
        source: "assign".into(),
        target: "output".into(),
        source_handle: Some("error".into()),
    });
    let routed_contract =
        WorkflowContract::from_spec(routed_workflow.clone()).expect("routed workflow");
    let routed_input_descriptor = descriptor(
        "workflow.input",
        WorkflowStepKind::Input,
        "invocation",
        "value",
    );
    let mut routed_assignment_descriptor = assignment_descriptor.clone();
    routed_assignment_descriptor.revision = "1.1.0".into();
    routed_assignment_descriptor.failure = WorkflowStepFailureContract {
        error_output: Some(WorkflowStepPort {
            name: "error".into(),
            value_type: WorkflowDataType::Object,
            cardinality: WorkflowStepPortCardinality::Single,
            required: true,
            dynamic: false,
        }),
        retry_classification: WorkflowStepRetryClassification::OwnerClassified,
        fallback: WorkflowStepFallbackMode::FailureBranch,
        failure_branch: true,
    };
    let routed_output_descriptor = descriptor(
        "workflow.output",
        WorkflowStepKind::Output,
        "result",
        "value",
    );
    let routed_registry =
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            id: "support.application-variable-routed".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            descriptors: vec![
                routed_input_descriptor,
                routed_assignment_descriptor,
                routed_output_descriptor,
            ],
        })
        .expect("routed registry");
    let routed_bindings =
        WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
            id: "support.application-variable-routed".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            bindings: [
                ("input", "workflow.input", "1.0.0"),
                (
                    "assign",
                    "application.conversation-variable-assign",
                    "1.1.0",
                ),
                ("output", "workflow.output", "1.0.0"),
            ]
            .into_iter()
            .map(
                |(step_id, descriptor_id, descriptor_revision)| WorkflowStepDescriptorBinding {
                    step_id: step_id.into(),
                    descriptor_id: descriptor_id.into(),
                    descriptor_revision: descriptor_revision.into(),
                    semantic_digest: routed_registry
                        .resolve(descriptor_id, descriptor_revision)
                        .expect("routed descriptor")
                        .semantic_digest()
                        .clone(),
                },
            )
            .collect(),
        })
        .expect("routed bindings");
    let routed_semantics = WorkflowRevisionSemanticContracts::create(
        &routed_workflow,
        routed_bindings,
        routed_registry,
        revision
            .semantic_contracts
            .as_ref()
            .expect("v12 semantic contracts")
            .variable_contract()
            .clone(),
    )
    .expect("routed semantic contracts");
    let routed_revision = WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        routed_revision_id,
        routed_contract.clone(),
        revision.payloads.clone(),
        routed_semantics,
        principal_id,
        now,
    )
    .expect("routed revision");
    let routed_definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        routed_workflow.name.clone(),
        routed_workflow.description.clone(),
        routed_revision_id,
        routed_contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("routed definition");
    let routed_goal = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "Routed Application variable goal".into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: routed_revision_id,
        workflow_digest: routed_contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: None,
        input: json!({
            "topic": "urgent",
            "conversationRevision": 0,
            "conversationEffect": "effect-1"
        }),
    })
    .expect("routed goal");
    let routed_compiled = WorkflowPlanCompiler::compile_goal(
        WorkflowGoalId::new(),
        PlanRevisionId::new(),
        routed_goal,
        &routed_definition,
        &routed_revision,
        &ontology_revision,
        principal_id,
        now,
    )
    .expect("compiled routed goal");
    assert_eq!(
        WorkflowPlanCompiler::compiler_revision(&routed_revision),
        WORKFLOW_PLAN_COMPILER_REVISION_V6
    );
    assert_eq!(
        routed_compiled.plan_revision.plan.schema,
        WORKFLOW_PLAN_SCHEMA_V6
    );
    let routed_run = WorkflowRunCompiler::compile_for_application(
        WorkflowRunId::new(),
        &routed_compiled.goal,
        &routed_compiled.plan_revision,
        &routed_revision,
        None,
        principal_id,
        now,
    )
    .expect("routed Application variable run");
    assert_eq!(
        routed_run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V14
    );
    assert_eq!(
        routed_run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14
    );
    assert_eq!(
        routed_run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V14
    );
    routed_run
        .run
        .execution_input
        .validate()
        .expect("valid v14 compiler output");

    assignment_descriptor.id = "application.variable-assign".into();
    assignment_descriptor.semantic_profile = "application.variable-assign".into();
    let alias_registry =
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            id: "support.application-variable-alias".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            descriptors: vec![
                descriptor(
                    "workflow.input",
                    WorkflowStepKind::Input,
                    "invocation",
                    "value",
                ),
                assignment_descriptor,
                descriptor(
                    "workflow.output",
                    WorkflowStepKind::Output,
                    "result",
                    "value",
                ),
            ],
        })
        .expect("alias registry");
    let alias_bindings =
        WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
            id: "support.application-variable-alias".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            bindings: [
                ("input", "workflow.input"),
                ("assign", "application.variable-assign"),
                ("output", "workflow.output"),
            ]
            .into_iter()
            .map(|(step_id, descriptor_id)| WorkflowStepDescriptorBinding {
                step_id: step_id.into(),
                descriptor_id: descriptor_id.into(),
                descriptor_revision: "1.0.0".into(),
                semantic_digest: alias_registry
                    .resolve(descriptor_id, "1.0.0")
                    .expect("descriptor")
                    .semantic_digest()
                    .clone(),
            })
            .collect(),
        })
        .expect("alias bindings");
    let error = WorkflowRevisionSemanticContracts::create(
        &bound_workflow,
        alias_bindings,
        alias_registry,
        revision
            .semantic_contracts
            .as_ref()
            .expect("semantics")
            .variable_contract()
            .clone(),
    )
    .expect_err("descriptor alias");
    assert!(error.contains("unsupported release_reference binding"));
}
