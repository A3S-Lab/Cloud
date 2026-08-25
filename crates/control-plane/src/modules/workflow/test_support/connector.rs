use super::*;

pub(crate) fn connector_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let goal_input = serde_json::json!({"ticketId": "T-42", "priority": "high"});
    let input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &goal_input,
        1024 * 1024,
        "WorkflowRun Connector test input",
    )?))?;
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Input))?;
    let connector_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Service))?;
    let output_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Output))?;
    let retry_policy = WorkflowPolicy {
        mode: WorkflowPolicyMode::Static,
        expression: None,
        candidates: Vec::new(),
        retry: Some(WorkflowRetryPolicy {
            maximum_attempts: 3,
            default_delay_seconds: 5,
        }),
        default_output: None,
    };
    let retry_payload =
        WorkflowPayload::from_content(WorkflowPayloadContent::Policy(retry_policy))?;
    let schema_digest = data_schema.digest().clone();
    let mut payloads = vec![
        data_schema,
        input_configuration.clone(),
        connector_configuration.clone(),
        output_configuration.clone(),
        retry_payload.clone(),
    ];
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let workflow_payload_set_digest = digest_payload_set(&payloads)?;
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.connector".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![WorkflowVariableDeclaration {
            name: "request".into(),
            scope: WorkflowVariableScope::InvocationInput,
            value_type: WorkflowDataType::Any,
            value_schema_digest: schema_digest.clone(),
            source_schema_digest: Some(schema_digest.clone()),
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
    })?;
    let semantic_digest = Sha256Digest::parse(digest('8'))?;
    let descriptor = |id: &str, descriptor_id: &str| WorkflowStepDescriptorBinding {
        step_id: id.into(),
        descriptor_id: descriptor_id.into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: semantic_digest.clone(),
    };
    let mut input_step = plan_step(
        "input",
        WorkflowStepKind::Input,
        &input_configuration,
        &schema_digest,
    );
    input_step.descriptor = Some(descriptor("input", "workflow.input"));
    let mut connector_step = plan_step(
        TEST_CONNECTOR_STEP_ID,
        WorkflowStepKind::Service,
        &connector_configuration,
        &schema_digest,
    );
    connector_step.descriptor = Some(descriptor(TEST_CONNECTOR_STEP_ID, "connector.http"));
    connector_step.policy_digest = Some(retry_payload.digest().clone());
    connector_step.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Connectors,
        capability_type: CapabilityType::ConnectorRevision,
        resource_id: ConnectorProfileId::new().as_uuid(),
        revision: ConnectorRevisionId::new().to_string(),
        digest: Sha256Digest::parse(digest('c'))?,
        capability: "connector.http".into(),
    });
    let mut output_step = plan_step(
        "output",
        WorkflowStepKind::Output,
        &output_configuration,
        &schema_digest,
    );
    output_step.descriptor = Some(descriptor("output", "workflow.output"));
    let plan = WorkflowPlan {
        schema: WORKFLOW_PLAN_SCHEMA_V2.into(),
        compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION_V2.into(),
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_digest: Sha256Digest::parse(digest('1'))?,
        workflow_payload_set_digest,
        semantic_contract_set_digest: Some(Sha256Digest::parse(digest('9'))?),
        variable_contract_digest: Some(variables.digest().clone()),
        composite_regions_digest: None,
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: Sha256Digest::parse(digest('2'))?,
        environment_id: Some(EnvironmentId::new()),
        input_digest,
        steps: vec![input_step, connector_step, output_step],
        edges: vec![
            edge("input-invoke", "input", TEST_CONNECTOR_STEP_ID, None),
            edge("invoke-output", TEST_CONNECTOR_STEP_ID, "output", None),
        ],
    };
    plan.validate()?;
    variables.validate_graph_bindings(&plan.workflow_spec()?)?;
    let plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun Connector test plan",
    )?))?;
    let input = WorkflowRunInput {
        schema: WORKFLOW_RUN_INPUT_SCHEMA_V8.into(),
        runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8.into(),
        flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
        flow_workflow_version: WORKFLOW_RUN_FLOW_VERSION_V8.into(),
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        workflow_run_id: WorkflowRunId::new(),
        workflow_goal_id: WorkflowGoalId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest,
        plan,
        goal_input,
        payloads: payloads
            .iter()
            .map(ResolvedWorkflowPayload::from_payload)
            .collect(),
        variable_contract: Some(
            crate::modules::workflow::domain::ResolvedWorkflowVariableContract::from_contract(
                &variables,
            ),
        ),
        variable_defaults: None,
        composite_regions: None,
        application_projection: None,
        requested_at: timestamp(8, 0),
        deadline_at: timestamp(9, 0),
    };
    input.validate()?;
    Ok(input)
}

pub(crate) fn routed_connector_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = connector_workflow_run_input()?;
    let output_step = input
        .plan
        .steps
        .iter()
        .find(|step| step.id == "output")
        .cloned()
        .ok_or_else(|| "WorkflowRun Connector test plan lost output".to_owned())?;
    let mut failure_output_step = output_step;
    failure_output_step.id = "failure_output".into();
    failure_output_step.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: failure_output_step.id.clone(),
        descriptor_id: "workflow.output".into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: Sha256Digest::parse(digest('8'))?,
    });
    input.plan.steps.insert(2, failure_output_step);
    for step in &mut input.plan.steps {
        step.failure = Some(if step.id == TEST_CONNECTOR_STEP_ID {
            routed_failure_contract()
        } else {
            unsupported_failure_contract()
        });
    }
    input.plan.edges = vec![
        edge("input-invoke", "input", TEST_CONNECTOR_STEP_ID, None),
        edge(
            "invoke-failure",
            TEST_CONNECTOR_STEP_ID,
            "failure_output",
            Some("error"),
        ),
        edge("invoke-output", TEST_CONNECTOR_STEP_ID, "output", None),
    ];
    input.plan.schema = WORKFLOW_PLAN_SCHEMA_V5.into();
    input.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V5.into();
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun routed Connector test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V9.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V9.into();
    input.validate()?;
    Ok(input)
}

pub(crate) fn compensating_connector_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = connector_workflow_run_input()?;
    let input_step = input
        .plan
        .steps
        .iter()
        .find(|step| step.id == "input")
        .cloned()
        .ok_or_else(|| "WorkflowRun compensation test plan lost its Input step".to_owned())?;
    let connector_step = input
        .plan
        .steps
        .iter()
        .find(|step| step.id == TEST_CONNECTOR_STEP_ID)
        .cloned()
        .ok_or_else(|| "WorkflowRun compensation test plan lost its Connector step".to_owned())?;
    let output_step = input
        .plan
        .steps
        .iter()
        .find(|step| step.id == "output")
        .cloned()
        .ok_or_else(|| "WorkflowRun compensation test plan lost its Output step".to_owned())?;

    let mut branch_configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Branch);
    branch_configuration.selector = Some("current.ok".into());
    branch_configuration.default_handle = Some("compensate".into());
    branch_configuration.routes = vec![
        WorkflowBranchRoute {
            handle: "complete".into(),
            equals: "true".into(),
        },
        WorkflowBranchRoute {
            handle: "compensate".into(),
            equals: "false".into(),
        },
    ];
    let branch_configuration = configuration(branch_configuration)?;
    let mut failure_output_configuration =
        WorkflowStepConfiguration::empty(WorkflowStepKind::Output);
    failure_output_configuration.template = Some("{{steps.charge}}".into());
    let failure_output_configuration = configuration(failure_output_configuration)?;
    input.payloads.extend([
        ResolvedWorkflowPayload::from_payload(&branch_configuration),
        ResolvedWorkflowPayload::from_payload(&failure_output_configuration),
    ]);
    input
        .payloads
        .sort_by(|left, right| left.digest.cmp(&right.digest));
    let restored_payloads = input
        .payloads
        .iter()
        .map(ResolvedWorkflowPayload::restore)
        .collect::<Result<Vec<_>, _>>()?;
    input.plan.workflow_payload_set_digest = digest_payload_set(&restored_payloads)?;

    let reserve = compensation_connector_step(&connector_step, "reserve", 'c')?;
    let charge = compensation_connector_step(&connector_step, "charge", 'd')?;
    let release = compensation_connector_step(&connector_step, "release", 'e')?;
    let mut route = plan_step(
        "route_charge",
        WorkflowStepKind::Branch,
        &branch_configuration,
        &connector_step.output_schema_digest,
    );
    route.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: route.id.clone(),
        descriptor_id: "workflow.branch".into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: Sha256Digest::parse(digest('8'))?,
    });
    let mut failure_output = compensation_output_step(&output_step, "failure_output")?;
    failure_output.configuration_digest = failure_output_configuration.digest().clone();
    let compensation_output = compensation_output_step(&output_step, "compensation_output")?;
    let success_output = compensation_output_step(&output_step, "success_output")?;

    input.plan.steps = vec![
        input_step,
        reserve,
        charge,
        route,
        release,
        compensation_output,
        failure_output,
        success_output,
    ];
    input.plan.edges = vec![
        edge("input-reserve", "input", "reserve", None),
        edge("reserve-charge", "reserve", "charge", None),
        edge("charge-route", "charge", "route_charge", None),
        edge(
            "route-release",
            "route_charge",
            "release",
            Some("compensate"),
        ),
        edge(
            "route-success-output",
            "route_charge",
            "success_output",
            Some("complete"),
        ),
        edge(
            "release-compensation-output",
            "release",
            "compensation_output",
            None,
        ),
        edge("release-failure-output", "release", "failure_output", None),
    ];
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun compensation test plan",
    )?))?;
    input.validate()?;
    Ok(input)
}

fn compensation_connector_step(
    base: &WorkflowPlanStep,
    step_id: &str,
    digest_character: char,
) -> Result<WorkflowPlanStep, String> {
    let mut step = base.clone();
    step.id = step_id.into();
    step.descriptor
        .as_mut()
        .ok_or_else(|| "WorkflowRun compensation Connector lost its descriptor".to_owned())?
        .step_id = step_id.into();
    let capability = step
        .capability
        .as_mut()
        .ok_or_else(|| "WorkflowRun compensation Connector lost its capability".to_owned())?;
    capability.resource_id = ConnectorProfileId::new().as_uuid();
    capability.revision = ConnectorRevisionId::new().to_string();
    capability.digest = Sha256Digest::parse(digest(digest_character))?;
    Ok(step)
}

fn compensation_output_step(
    base: &WorkflowPlanStep,
    step_id: &str,
) -> Result<WorkflowPlanStep, String> {
    let mut step = base.clone();
    step.id = step_id.into();
    step.descriptor
        .as_mut()
        .ok_or_else(|| "WorkflowRun compensation Output lost its descriptor".to_owned())?
        .step_id = step_id.into();
    Ok(step)
}

pub(crate) fn connector_workflow_run_input_v6() -> Result<WorkflowRunInput, String> {
    let mut input = connector_workflow_run_input()?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V6.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V6.into();
    input.validate()?;
    Ok(input)
}

pub(crate) fn connector_workflow_run_input_v5() -> Result<WorkflowRunInput, String> {
    let mut input = connector_workflow_run_input_v6()?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V5.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V5.into();
    input.validate()?;
    Ok(input)
}
