use super::*;

struct FailureRouteFixture {
    workflow: WorkflowSpec,
    payloads: Vec<WorkflowPayload>,
    semantic_contracts: WorkflowRevisionSemanticContracts,
}

fn failure_route_fixture() -> FailureRouteFixture {
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
    let execution_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Execution),
        ))
        .expect("execution configuration");
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))
        .expect("output configuration");
    let mut execution = step("execute", WorkflowStepKind::Execution);
    execution.configuration_digest = execution_configuration.digest().clone();
    execution.input_schema_digest = data_schema.digest().clone();
    execution.output_schema_digest = data_schema.digest().clone();
    execution.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Executions,
        capability_type: CapabilityType::ExecutionTemplate,
        resource_id: uuid::Uuid::now_v7(),
        revision: uuid::Uuid::now_v7().to_string(),
        digest: digest('e'),
        capability: "execution.run".into(),
    });
    let with_payload =
        |mut step: WorkflowStepSpec, configuration: &WorkflowPayload| -> WorkflowStepSpec {
            step.configuration_digest = configuration.digest().clone();
            step.input_schema_digest = data_schema.digest().clone();
            step.output_schema_digest = data_schema.digest().clone();
            step
        };
    let workflow = WorkflowSpec {
        name: "Routed execution".into(),
        description: String::new(),
        steps: vec![
            with_payload(step("input", WorkflowStepKind::Input), &input_configuration),
            execution,
            with_payload(
                step("failure_output", WorkflowStepKind::Output),
                &output_configuration,
            ),
            with_payload(
                step("output", WorkflowStepKind::Output),
                &output_configuration,
            ),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-execute".into(),
                source: "input".into(),
                target: "execute".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "execute-failure".into(),
                source: "execute".into(),
                target: "failure_output".into(),
                source_handle: Some("error".into()),
            },
            WorkflowEdgeSpec {
                id: "execute-output".into(),
                source: "execute".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };

    let mut execution_descriptor = descriptor(
        "executions.finite",
        WorkflowStepKind::Execution,
        "input",
        "result",
    );
    execution_descriptor.owner = WorkflowStepOwner::Executions;
    execution_descriptor.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
    execution_descriptor.required_bindings = vec![WorkflowStepBindingKind::CapabilityReference];
    execution_descriptor.allowed_capability_types = vec![CapabilityType::ExecutionTemplate];
    execution_descriptor.failure = WorkflowStepFailureContract {
        error_output: Some(port("error")),
        retry_classification: WorkflowStepRetryClassification::OwnerClassified,
        fallback: WorkflowStepFallbackMode::FailureBranch,
        failure_branch: true,
    };
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.failure-route".into(),
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
    .expect("failure route registry");
    let descriptor_ids = [
        ("input", "workflow.input"),
        ("execute", "executions.finite"),
        ("failure_output", "workflow.output"),
        ("output", "workflow.output"),
    ];
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "support.failure-route".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: descriptor_ids
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
    .expect("failure route bindings");
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.failure-route".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![WorkflowVariableDeclaration {
            name: "request".into(),
            scope: WorkflowVariableScope::InvocationInput,
            value_type: WorkflowDataType::Object,
            value_schema_digest: data_schema.digest().clone(),
            source_schema_digest: Some(data_schema.digest().clone()),
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required: true,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: None,
            default_value_digest: None,
        }],
        reads: Vec::new(),
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .expect("failure route variables");
    let semantic_contracts =
        WorkflowRevisionSemanticContracts::create(&workflow, bindings, registry, variables)
            .expect("failure route semantics");
    FailureRouteFixture {
        workflow,
        payloads: vec![
            data_schema,
            input_configuration,
            execution_configuration,
            output_configuration,
        ],
        semantic_contracts,
    }
}

fn default_output_fixture() -> FailureRouteFixture {
    let mut fixture = failure_route_fixture();
    fixture
        .workflow
        .steps
        .retain(|step| step.id != "failure_output");
    fixture
        .workflow
        .edges
        .retain(|edge| edge.id != "execute-failure");
    let policy = WorkflowPayload::from_content(WorkflowPayloadContent::Policy(WorkflowPolicy {
        mode: WorkflowPolicyMode::Static,
        expression: None,
        candidates: Vec::new(),
        retry: None,
        default_output: Some(
            WorkflowDefaultOutput::new("result", json!({"status": "temporarily_unavailable"}))
                .expect("default output"),
        ),
    }))
    .expect("default-output policy");
    fixture
        .workflow
        .steps
        .iter_mut()
        .find(|step| step.id == "execute")
        .expect("Execution step")
        .policy_digest = Some(policy.digest().clone());

    let old_registry = fixture.semantic_contracts.descriptor_registry();
    let mut descriptor_specs = old_registry
        .descriptors()
        .iter()
        .map(|descriptor| descriptor.spec().clone())
        .collect::<Vec<_>>();
    let execution = descriptor_specs
        .iter_mut()
        .find(|descriptor| descriptor.id == "executions.finite")
        .expect("Execution descriptor");
    execution.default_policy_digest = Some(policy.digest().clone());
    execution.failure = WorkflowStepFailureContract {
        error_output: None,
        retry_classification: WorkflowStepRetryClassification::OwnerClassified,
        fallback: WorkflowStepFallbackMode::DefaultOutput,
        failure_branch: false,
    };
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: old_registry.id().into(),
        revision: old_registry.revision().into(),
        compiler_schema_version: old_registry.compiler_schema_version(),
        descriptors: descriptor_specs,
    })
    .expect("default-output registry");
    let old_bindings = fixture.semantic_contracts.descriptor_bindings();
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: old_bindings.id().into(),
        revision: old_bindings.revision().into(),
        compiler_schema_version: old_bindings.compiler_schema_version(),
        bindings: old_bindings
            .bindings()
            .iter()
            .filter(|binding| binding.step_id != "failure_output")
            .map(|binding| WorkflowStepDescriptorBinding {
                step_id: binding.step_id.clone(),
                descriptor_id: binding.descriptor_id.clone(),
                descriptor_revision: binding.descriptor_revision.clone(),
                semantic_digest: registry
                    .resolve(&binding.descriptor_id, &binding.descriptor_revision)
                    .expect("rebuilt descriptor")
                    .semantic_digest()
                    .clone(),
            })
            .collect(),
    })
    .expect("default-output bindings");
    let variables = fixture.semantic_contracts.variable_contract().clone();
    fixture.semantic_contracts =
        WorkflowRevisionSemanticContracts::create(&fixture.workflow, bindings, registry, variables)
            .expect("default-output semantics");
    fixture.payloads.push(policy);
    fixture
}

#[test]
fn default_output_semantics_require_exact_execution_policy_material() {
    let fixture = default_output_fixture();
    fixture
        .semantic_contracts
        .validate(&fixture.workflow)
        .expect("default-output semantics");
    let mut drifted_policy = fixture.workflow.clone();
    drifted_policy
        .steps
        .iter_mut()
        .find(|step| step.id == "execute")
        .expect("Execution step")
        .policy_digest = Some(digest('0'));
    let error = fixture
        .semantic_contracts
        .validate(&drifted_policy)
        .expect_err("descriptor policy drift");
    assert!(error.contains("exact descriptor policy"));
    let contract = fixture
        .semantic_contracts
        .default_output_contract("execute")
        .expect("default-output contract")
        .expect("Execution fallback");
    assert_eq!(contract.output_port.name, "result");

    let workflow_contract =
        WorkflowContract::from_spec(fixture.workflow.clone()).expect("workflow contract");
    let revision = WorkflowRevision::initial_with_semantic_contracts(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowDefinitionId::new(),
        WorkflowRevisionId::new(),
        workflow_contract,
        fixture.payloads,
        fixture.semantic_contracts,
        PrincipalId::new(),
        Utc::now(),
    )
    .expect("default-output revision");
    assert_eq!(
        WorkflowPlanCompiler::compiler_revision(&revision),
        WORKFLOW_PLAN_COMPILER_REVISION_V4
    );

    let mut missing_material = revision.clone();
    missing_material
        .payloads
        .retain(|payload| payload.kind() != WorkflowPayloadKind::Policy);
    assert!(missing_material.validate().is_err());
}

#[test]
fn semantic_contracts_require_the_exact_descriptor_error_handle() {
    let fixture = failure_route_fixture();
    fixture
        .semantic_contracts
        .validate(&fixture.workflow)
        .expect("exact error handle");
    let mut drifted = fixture.workflow;
    drifted
        .edges
        .iter_mut()
        .find(|edge| edge.id == "execute-failure")
        .expect("failure edge")
        .source_handle = Some("other_error".into());
    let error = fixture
        .semantic_contracts
        .validate(&drifted)
        .expect_err("drifted error handle");
    assert!(error.contains("does not match descriptor error output"));
}

#[test]
fn compiler_emits_plan_v3_and_run_v4_for_descriptor_bound_failure_routes() {
    let (compiled, revision, principal_id, now) =
        compile_execution_fallback_fixture(failure_route_fixture(), "Routed goal");
    assert_eq!(compiled.plan_revision.plan.schema, WORKFLOW_PLAN_SCHEMA_V3);
    assert_eq!(
        compiled.plan_revision.plan.compiler_revision,
        WORKFLOW_PLAN_COMPILER_REVISION_V3
    );
    assert!(compiled
        .plan_revision
        .plan
        .steps
        .iter()
        .all(|step| step.failure.is_some()));
    let mut drifted_handle = compiled.plan_revision.plan.clone();
    drifted_handle
        .steps
        .iter_mut()
        .find(|step| step.id == "execute")
        .and_then(|step| step.failure.as_mut())
        .and_then(|failure| failure.error_output.as_mut())
        .expect("execution error output")
        .name = "other_error".into();
    assert!(drifted_handle.validate().is_err());
    let mut incomplete_contracts = compiled.plan_revision.plan.clone();
    incomplete_contracts
        .steps
        .iter_mut()
        .find(|step| step.id == "output")
        .expect("output step")
        .failure = None;
    assert!(incomplete_contracts.validate().is_err());
    let run = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("compiled run v4");
    assert_eq!(run.run.execution_input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V4);
    assert_eq!(
        run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4
    );
    assert_eq!(
        run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V4
    );
    run.run
        .execution_input
        .validate()
        .expect("valid run v4 input");
}

fn compile_execution_fallback_fixture(
    fixture: FailureRouteFixture,
    goal_name: &str,
) -> (
    CompiledWorkflowGoal,
    WorkflowRevision,
    PrincipalId,
    chrono::DateTime<Utc>,
) {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let principal_id = PrincipalId::new();
    let now = Utc::now();
    let contract = WorkflowContract::from_spec(fixture.workflow.clone()).expect("workflow");
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
        fixture.workflow.name,
        fixture.workflow.description,
        revision_id,
        contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Failure ontology".into(),
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
    let goal = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: goal_name.into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: revision_id,
        workflow_digest: contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: Some(EnvironmentId::new()),
        input: json!({}),
    })
    .expect("goal");
    let compiled = WorkflowPlanCompiler::compile_goal(
        WorkflowGoalId::new(),
        PlanRevisionId::new(),
        goal,
        &definition,
        &revision,
        &ontology_revision,
        principal_id,
        now,
    )
    .expect("compiled execution fallback");
    (compiled, revision, principal_id, now)
}

#[test]
fn compiler_emits_plan_v4_and_run_v7_for_exact_default_output_fallback() {
    let (compiled, revision, principal_id, now) =
        compile_execution_fallback_fixture(default_output_fixture(), "Default-output goal");
    assert_eq!(compiled.plan_revision.plan.schema, WORKFLOW_PLAN_SCHEMA_V4);
    assert_eq!(
        compiled.plan_revision.plan.compiler_revision,
        WORKFLOW_PLAN_COMPILER_REVISION_V4
    );
    assert_eq!(
        compiled
            .plan_revision
            .plan
            .steps
            .iter()
            .filter(|step| step.default_output.is_some())
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        vec!["execute"]
    );
    let mut drifted = compiled.plan_revision.plan.clone();
    drifted
        .steps
        .iter_mut()
        .find(|step| step.id == "execute")
        .and_then(|step| step.default_output.as_mut())
        .expect("default-output contract")
        .output_port
        .name = "other_result".into();
    assert!(revision
        .semantic_contracts
        .as_ref()
        .expect("semantic contracts")
        .validate_plan_bindings(&drifted)
        .is_err());
    let mut drifted_policy = compiled.plan_revision.plan.clone();
    drifted_policy
        .steps
        .iter_mut()
        .find(|step| step.id == "execute")
        .expect("Execution step")
        .policy_digest = Some(digest('0'));
    assert!(revision
        .semantic_contracts
        .as_ref()
        .expect("semantic contracts")
        .validate_plan_bindings(&drifted_policy)
        .is_err());
    let run = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("compiled run v7");
    assert_eq!(run.run.execution_input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V7);
    assert_eq!(
        run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7
    );
    assert_eq!(
        run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V7
    );
    run.run
        .execution_input
        .validate()
        .expect("valid run v7 input");
}
