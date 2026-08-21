use super::*;

pub(crate) fn application_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = typed_variable_workflow_run_input()?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V10.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V10.into();
    input.application_projection = Some(WorkflowRunApplicationProjection::from_plan(&input.plan)?);
    input.validate()?;
    Ok(input)
}

pub(crate) fn application_answer_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = typed_variable_workflow_run_input()?;
    let output = input
        .plan
        .steps
        .iter()
        .find(|step| step.id == "output")
        .cloned()
        .ok_or_else(|| "WorkflowRun test plan has no Output step".to_owned())?;
    let mut answer = output;
    answer.id = TEST_ANSWER_STEP_ID.into();
    let descriptor = answer
        .descriptor
        .as_mut()
        .ok_or_else(|| "WorkflowRun test Answer has no descriptor binding".to_owned())?;
    descriptor.step_id = TEST_ANSWER_STEP_ID.into();
    descriptor.descriptor_id = "application.answer".into();
    let output_index = input
        .plan
        .steps
        .iter()
        .position(|step| step.id == "output")
        .ok_or_else(|| "WorkflowRun test plan has no Output position".to_owned())?;
    input.plan.steps.insert(output_index, answer);
    input.plan.edges.extend([
        edge("high-answer", "high", TEST_ANSWER_STEP_ID, None),
        edge("normal-answer", "normal", TEST_ANSWER_STEP_ID, None),
    ]);
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V11.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V11.into();
    input.application_projection =
        Some(WorkflowRunApplicationProjection::from_application_outputs(
            &input.plan,
            "output".into(),
            vec![TEST_ANSWER_STEP_ID.into()],
        )?);
    input.validate()?;
    Ok(input)
}

pub(crate) fn application_answers_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = application_answer_workflow_run_input()?;
    let mut second_answer = input
        .plan
        .steps
        .iter()
        .find(|step| step.id == TEST_ANSWER_STEP_ID)
        .cloned()
        .ok_or_else(|| "WorkflowRun test plan has no first Answer step".to_owned())?;
    second_answer.id = TEST_SECOND_ANSWER_STEP_ID.into();
    second_answer
        .descriptor
        .as_mut()
        .ok_or_else(|| "WorkflowRun test second Answer has no descriptor binding".to_owned())?
        .step_id = TEST_SECOND_ANSWER_STEP_ID.into();
    let output_index = input
        .plan
        .steps
        .iter()
        .position(|step| step.id == "output")
        .ok_or_else(|| "WorkflowRun test plan has no Output position".to_owned())?;
    input.plan.steps.insert(output_index, second_answer);
    input.plan.edges.extend([
        edge(
            "high-answer-second",
            "high",
            TEST_SECOND_ANSWER_STEP_ID,
            None,
        ),
        edge(
            "normal-answer-second",
            "normal",
            TEST_SECOND_ANSWER_STEP_ID,
            None,
        ),
    ]);
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun test plan",
    )?))?;
    input.application_projection =
        Some(WorkflowRunApplicationProjection::from_application_outputs(
            &input.plan,
            "output".into(),
            vec![
                TEST_ANSWER_STEP_ID.into(),
                TEST_SECOND_ANSWER_STEP_ID.into(),
            ],
        )?);
    input.validate()?;
    Ok(input)
}

pub(crate) fn application_variable_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = typed_variable_workflow_run_input()?;
    let schema_digest = input
        .plan
        .steps
        .first()
        .ok_or_else(|| "WorkflowRun test plan has no steps".to_owned())?
        .output_schema_digest
        .clone();
    let assignment_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Service))?;
    let mut assignment = plan_step(
        TEST_APPLICATION_VARIABLE_STEP_ID,
        WorkflowStepKind::Service,
        &assignment_configuration,
        &schema_digest,
    );
    let semantic_digest = input
        .plan
        .steps
        .first()
        .and_then(|step| step.descriptor.as_ref())
        .ok_or_else(|| "WorkflowRun test plan has no descriptor binding".to_owned())?
        .semantic_digest
        .clone();
    assignment.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: TEST_APPLICATION_VARIABLE_STEP_ID.into(),
        descriptor_id: "application.conversation-variable-assign".into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest,
    });
    let output_index = input
        .plan
        .steps
        .iter()
        .position(|step| step.id == "output")
        .ok_or_else(|| "WorkflowRun test plan has no output step".to_owned())?;
    input.plan.steps.insert(output_index, assignment);
    input
        .plan
        .edges
        .retain(|edge| edge.id != "high-output" && edge.id != "normal-output");
    input.plan.edges.extend([
        edge(
            "high-assign-conversation",
            "high",
            TEST_APPLICATION_VARIABLE_STEP_ID,
            None,
        ),
        edge(
            "normal-assign-conversation",
            "normal",
            TEST_APPLICATION_VARIABLE_STEP_ID,
            None,
        ),
        edge(
            "assign-conversation-output",
            TEST_APPLICATION_VARIABLE_STEP_ID,
            "output",
            None,
        ),
    ]);

    let existing = input
        .variable_contract
        .as_ref()
        .ok_or_else(|| "WorkflowRun test plan has no variable contract".to_owned())?
        .restore()?;
    let mut variables = existing.spec().clone();
    variables.declarations.extend([
        WorkflowVariableDeclaration {
            name: "conversation_topic".into(),
            scope: WorkflowVariableScope::Application,
            value_type: WorkflowDataType::String,
            value_schema_digest: schema_digest.clone(),
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
            value_schema_digest: schema_digest.clone(),
            source_schema_digest: Some(schema_digest.clone()),
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required: false,
            source_step_id: None,
            source_path: vec!["conversationRevision".into()],
            region_id: None,
            default_value_digest: None,
        },
        WorkflowVariableDeclaration {
            name: "conversation_effect".into(),
            scope: WorkflowVariableScope::InvocationInput,
            value_type: WorkflowDataType::String,
            value_schema_digest: schema_digest.clone(),
            source_schema_digest: Some(schema_digest.clone()),
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required: false,
            source_step_id: None,
            source_path: vec!["conversationEffect".into()],
            region_id: None,
            default_value_digest: None,
        },
    ]);
    variables.assignments.push(WorkflowVariableAssignment {
        id: "assign-conversation-topic".into(),
        target_variable: "conversation_topic".into(),
        source_variable: "request".into(),
        writer_step_id: TEST_APPLICATION_VARIABLE_STEP_ID.into(),
        writer_region_id: None,
        source_path: vec!["priority".into()],
        value_type: WorkflowDataType::String,
        value_schema_digest: schema_digest.clone(),
        mutation_order: 1,
        expected_revision_variable: Some("conversation_revision".into()),
        idempotency_key_variable: Some("conversation_effect".into()),
    });
    let variables = WorkflowVariableContract::from_spec(variables)?;

    let mut payloads = input
        .payloads
        .iter()
        .map(ResolvedWorkflowPayload::restore)
        .collect::<Result<Vec<_>, _>>()?;
    payloads.push(assignment_configuration);
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    input.plan.workflow_payload_set_digest = digest_payload_set(&payloads)?;
    input.payloads = payloads
        .iter()
        .map(ResolvedWorkflowPayload::from_payload)
        .collect();
    input.plan.variable_contract_digest = Some(variables.digest().clone());
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V12.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V12.into();
    input.variable_contract = Some(ResolvedWorkflowVariableContract::from_contract(&variables));
    input.application_projection = Some(
        WorkflowRunApplicationProjection::from_application_variables(
            &input.plan,
            "output".into(),
            Vec::new(),
            vec![TEST_APPLICATION_VARIABLE_STEP_ID.into()],
            vec![TEST_APPLICATION_VARIABLE_STEP_ID.into()],
        )?,
    );
    input.validate()?;
    Ok(input)
}
