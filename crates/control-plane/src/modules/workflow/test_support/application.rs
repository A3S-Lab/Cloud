use super::*;
use crate::modules::workflow::domain::{
    WorkflowApplicationFrameAuthority, WorkflowCompositeFrame, WorkflowIterationFailureMode,
    WorkflowIterationRegionPolicy,
};

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

pub(crate) fn application_frame_answer_workflow_run_input(
    ordinal: u32,
) -> Result<(WorkflowRunInput, WorkflowCompositeFrame, WorkflowRunInput), String> {
    let (parent, child) = application_frame_answer_parent()?;
    let (frame, child) = application_frame_answer_child(&parent, &child, ordinal)?;
    Ok((parent, frame, child))
}

pub(crate) fn application_frame_answer_workflow_run_inputs() -> Result<
    (
        WorkflowRunInput,
        Vec<(WorkflowCompositeFrame, WorkflowRunInput)>,
    ),
    String,
> {
    let (parent, child) = application_frame_answer_parent()?;
    let frames = (0..2)
        .map(|ordinal| application_frame_answer_child(&parent, &child, ordinal))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((parent, frames))
}

pub(crate) fn application_nested_frame_answer_authorities(
) -> Result<[WorkflowApplicationFrameAuthority; 3], String> {
    let (mut outer, _) = application_frame_answer_parent()?;
    let (middle, leaf) = application_frame_answer_parent()?;
    let middle_plan = &middle.plan;
    let capability = outer
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == "iteration")
        .and_then(|step| step.capability.as_mut())
        .ok_or_else(|| "nested Application root lost its child capability".to_owned())?;
    capability.resource_id = middle_plan.workflow_definition_id.as_uuid();
    capability.revision = middle_plan.workflow_revision_id.to_string();
    capability.digest = middle_plan.workflow_digest.clone();
    outer.plan.validate()?;
    outer.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &outer.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "nested Application root test plan",
    )?))?;
    outer.validate()?;

    let (_, middle_zero) = application_frame_answer_child(&outer, &middle, 0)?;
    let (_, middle_one) = application_frame_answer_child(&outer, &middle, 1)?;
    let (_, inner_zero_zero) = application_frame_answer_child(&middle_zero, &leaf, 0)?;
    let (_, inner_zero_one) = application_frame_answer_child(&middle_zero, &leaf, 1)?;
    let (_, inner_one_zero) = application_frame_answer_child(&middle_one, &leaf, 0)?;
    let authority = |input: &WorkflowRunInput| {
        input
            .application_projection
            .as_ref()
            .and_then(|projection| projection.frame_authority.clone())
            .ok_or_else(|| "nested Application child lost its frame authority".to_owned())
    };
    Ok([
        authority(&inner_zero_zero)?,
        authority(&inner_zero_one)?,
        authority(&inner_one_zero)?,
    ])
}

fn application_frame_answer_parent() -> Result<(WorkflowRunInput, WorkflowRunInput), String> {
    let child = application_answer_workflow_run_input()?;
    let policy = WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
        step_id: "iteration".into(),
        maximum_items: 2,
        maximum_concurrency: 1,
        failure_mode: WorkflowIterationFailureMode::Terminate,
    });
    let root_input = serde_json::json!({
        "items": [child.goal_input.clone(), child.goal_input.clone()]
    });
    let mut parent = composite_workflow_run_input(policy, root_input)?;
    let schema_digest = parent
        .plan
        .steps
        .first()
        .ok_or_else(|| "Application frame parent lost its Input step".to_owned())?
        .output_schema_digest
        .clone();
    let semantic_digest = parent
        .plan
        .steps
        .first()
        .and_then(|step| step.descriptor.as_ref())
        .ok_or_else(|| "Application frame parent lost its descriptor binding".to_owned())?
        .semantic_digest
        .clone();
    let mut items_configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    items_configuration.template = Some("{{current.items}}".into());
    let items_configuration = configuration(items_configuration)?;
    let mut items = plan_step(
        "items",
        WorkflowStepKind::Transform,
        &items_configuration,
        &schema_digest,
    );
    items.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: "items".into(),
        descriptor_id: "workflow.transform".into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest,
    });
    parent.plan.steps.insert(1, items);
    parent
        .plan
        .edges
        .retain(|edge| !(edge.source == "input" && edge.target == "iteration"));
    parent.plan.edges.extend([
        edge("input-items", "input", "items", None),
        edge("items-iteration", "items", "iteration", None),
    ]);
    parent
        .plan
        .edges
        .sort_by(|left, right| left.id.cmp(&right.id));
    parent
        .payloads
        .push(ResolvedWorkflowPayload::from_payload(&items_configuration));
    parent
        .payloads
        .sort_by(|left, right| left.digest.cmp(&right.digest));
    let payloads = parent
        .payloads
        .iter()
        .map(ResolvedWorkflowPayload::restore)
        .collect::<Result<Vec<_>, _>>()?;
    parent.plan.workflow_payload_set_digest = digest_payload_set(&payloads)?;
    {
        let composite = parent
            .plan
            .steps
            .iter_mut()
            .find(|step| step.id == "iteration")
            .ok_or_else(|| "Application frame parent lost its composite step".to_owned())?;
        let capability = composite
            .capability
            .as_mut()
            .ok_or_else(|| "Application frame parent lost its child capability".to_owned())?;
        capability.resource_id = child.plan.workflow_definition_id.as_uuid();
        capability.revision = child.plan.workflow_revision_id.to_string();
        capability.digest = child.plan.workflow_digest.clone();
    }
    parent.plan.validate()?;
    parent.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &parent.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "Application frame parent test plan",
    )?))?;
    parent.schema = crate::modules::workflow::domain::WORKFLOW_RUN_INPUT_SCHEMA_V13.into();
    parent.runtime_contract_revision =
        crate::modules::workflow::domain::WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13.into();
    parent.flow_workflow_version =
        crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V13.into();
    parent.application_projection = Some(
        WorkflowRunApplicationProjection::from_application_composite(
            &parent.plan,
            "output".into(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )?,
    );
    parent.validate()?;
    Ok((parent, child))
}

fn application_frame_answer_child(
    parent: &WorkflowRunInput,
    child: &WorkflowRunInput,
    ordinal: u32,
) -> Result<(WorkflowCompositeFrame, WorkflowRunInput), String> {
    let variables = parent
        .variable_contract
        .as_ref()
        .ok_or_else(|| "Application frame parent lost its variable contract".to_owned())?
        .restore()?;
    let regions = parent
        .composite_regions
        .as_ref()
        .ok_or_else(|| "Application frame parent lost its composite regions".to_owned())?
        .restore()?;
    let frame = WorkflowCompositeFrame::open(
        crate::modules::workflow::domain::WorkflowCompositeFrameRequest {
            organization_id: parent.organization_id,
            project_id: parent.project_id,
            workflow_run_id: parent.workflow_run_id,
            plan_revision_id: parent.plan_revision_id,
            plan_digest: parent.plan_digest.clone(),
            region_step_id: "iteration".into(),
            ordinal,
            effective_input: child.goal_input.clone(),
            available_variables: std::collections::BTreeMap::from([(
                "request".into(),
                parent.goal_input.clone(),
            )]),
        },
        &parent.plan,
        &regions,
        &variables,
        None,
    )?;
    let authority =
        crate::modules::workflow::domain::WorkflowApplicationFrameAuthority::from_parent(
            parent, &frame,
        )?
        .ok_or_else(|| "Application frame authority was not projected".to_owned())?;
    let mut child = child.clone();
    let current_projection = child
        .application_projection
        .take()
        .ok_or_else(|| "Application frame child lost its Answer projection".to_owned())?;
    child.organization_id = parent.organization_id;
    child.project_id = parent.project_id;
    child.workflow_run_id = frame.child_workflow_run_id();
    child.schema = crate::modules::workflow::domain::WORKFLOW_RUN_INPUT_SCHEMA_V13.into();
    child.runtime_contract_revision =
        crate::modules::workflow::domain::WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13.into();
    child.flow_workflow_version =
        crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V13.into();
    child.application_projection = Some(WorkflowRunApplicationProjection::from_application_frame(
        &child.plan,
        current_projection.final_output_step_id,
        current_projection.answer_step_ids,
        authority,
    )?);
    child.validate()?;
    Ok((frame, child))
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

pub(crate) fn routed_application_variable_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = application_variable_workflow_run_input()?;
    for step in &mut input.plan.steps {
        step.failure = Some(if step.id == TEST_APPLICATION_VARIABLE_STEP_ID {
            routed_failure_contract()
        } else {
            unsupported_failure_contract()
        });
    }
    input.plan.edges.push(edge(
        "assign-conversation-error-output",
        TEST_APPLICATION_VARIABLE_STEP_ID,
        "output",
        Some("error"),
    ));
    input
        .plan
        .edges
        .sort_by(|left, right| left.id.cmp(&right.id));
    input.plan.schema = WORKFLOW_PLAN_SCHEMA_V6.into();
    input.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V6.into();
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "routed Application variable WorkflowRun test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V14.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V14.into();
    input.validate()?;
    Ok(input)
}
