use super::*;

pub(crate) fn transform_failure_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = routed_execution_workflow_run_input()?;
    let transform_step = input
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_EXECUTION_STEP_ID)
        .ok_or_else(|| "WorkflowRun Transform test plan lost its Transform step".to_owned())?;
    let previous_configuration_digest = transform_step.configuration_digest.clone();
    let mut transform_configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    transform_configuration.template = Some("{{current.missing}}".into());
    let transform_configuration = configuration(transform_configuration)?;

    transform_step.kind = WorkflowStepKind::Transform;
    transform_step.configuration_digest = transform_configuration.digest().clone();
    transform_step.capability = None;
    transform_step.policy_digest = None;
    transform_step.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: TEST_EXECUTION_STEP_ID.into(),
        descriptor_id: "workflow.transform".into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: Sha256Digest::parse(digest('8'))?,
    });
    transform_step.failure = Some(WorkflowStepFailureContract {
        error_output: Some(WorkflowStepPort {
            name: "error".into(),
            value_type: WorkflowDataType::Object,
            cardinality: WorkflowStepPortCardinality::Single,
            required: true,
            dynamic: false,
        }),
        retry_classification: WorkflowStepRetryClassification::NotRetryable,
        fallback: WorkflowStepFallbackMode::FailureBranch,
        failure_branch: true,
    });

    input
        .payloads
        .retain(|payload| payload.digest != previous_configuration_digest);
    input.payloads.push(ResolvedWorkflowPayload::from_payload(
        &transform_configuration,
    ));
    input
        .payloads
        .sort_by(|left, right| left.digest.cmp(&right.digest));
    let restored_payloads = input
        .payloads
        .iter()
        .map(ResolvedWorkflowPayload::restore)
        .collect::<Result<Vec<_>, _>>()?;
    input.plan.workflow_payload_set_digest = digest_payload_set(&restored_payloads)?;
    input.plan.schema = WORKFLOW_PLAN_SCHEMA_V8.into();
    input.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V8.into();
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun routed Transform test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V16.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V16.into();
    input.validate()?;
    Ok(input)
}
