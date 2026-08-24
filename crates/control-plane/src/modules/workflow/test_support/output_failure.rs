use super::*;

pub(crate) fn output_failure_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = transform_failure_workflow_run_input()?;
    let output_step = input
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_EXECUTION_STEP_ID)
        .ok_or_else(|| "WorkflowRun Output test plan lost its Output step".to_owned())?;
    let previous_configuration_digest = output_step.configuration_digest.clone();
    let mut output_configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Output);
    output_configuration.template = Some("{{current.missing}}".into());
    let output_configuration = configuration(output_configuration)?;

    output_step.kind = WorkflowStepKind::Output;
    output_step.configuration_digest = output_configuration.digest().clone();
    output_step.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: TEST_EXECUTION_STEP_ID.into(),
        descriptor_id: "workflow.output".into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: Sha256Digest::parse(digest('9'))?,
    });

    input
        .payloads
        .retain(|payload| payload.digest != previous_configuration_digest);
    input
        .payloads
        .push(ResolvedWorkflowPayload::from_payload(&output_configuration));
    input
        .payloads
        .sort_by(|left, right| left.digest.cmp(&right.digest));
    let restored_payloads = input
        .payloads
        .iter()
        .map(ResolvedWorkflowPayload::restore)
        .collect::<Result<Vec<_>, _>>()?;
    input.plan.workflow_payload_set_digest = digest_payload_set(&restored_payloads)?;
    input.plan.schema = WORKFLOW_PLAN_SCHEMA_V9.into();
    input.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V9.into();
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun routed Output test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V17.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V17.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V17.into();
    input.validate()?;
    Ok(input)
}
