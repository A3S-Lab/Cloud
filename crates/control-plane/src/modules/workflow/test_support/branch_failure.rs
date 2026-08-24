use super::*;

pub(crate) fn branch_failure_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = routed_execution_workflow_run_input()?;
    let branch_step = input
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_EXECUTION_STEP_ID)
        .ok_or_else(|| "WorkflowRun Branch test plan lost its Branch step".to_owned())?;
    let previous_configuration_digest = branch_step.configuration_digest.clone();
    let mut branch_configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Branch);
    branch_configuration.selector = Some("current.missing".into());
    branch_configuration.routes = vec![WorkflowBranchRoute {
        handle: "matched".into(),
        equals: "matched".into(),
    }];
    branch_configuration.default_handle = Some("matched".into());
    let branch_configuration = configuration(branch_configuration)?;

    branch_step.kind = WorkflowStepKind::Branch;
    branch_step.configuration_digest = branch_configuration.digest().clone();
    branch_step.capability = None;
    branch_step.policy_digest = None;
    branch_step.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: TEST_EXECUTION_STEP_ID.into(),
        descriptor_id: "workflow.branch".into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: Sha256Digest::parse(digest('a'))?,
    });
    branch_step.failure = Some(WorkflowStepFailureContract {
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
        .plan
        .edges
        .iter_mut()
        .find(|edge| edge.id == "execute-output")
        .ok_or_else(|| "WorkflowRun Branch test plan lost its ordinary route".to_owned())?
        .source_handle = Some("matched".into());
    input
        .payloads
        .retain(|payload| payload.digest != previous_configuration_digest);
    input
        .payloads
        .push(ResolvedWorkflowPayload::from_payload(&branch_configuration));
    input
        .payloads
        .sort_by(|left, right| left.digest.cmp(&right.digest));
    let restored_payloads = input
        .payloads
        .iter()
        .map(ResolvedWorkflowPayload::restore)
        .collect::<Result<Vec<_>, _>>()?;
    input.plan.workflow_payload_set_digest = digest_payload_set(&restored_payloads)?;
    input.plan.schema = WORKFLOW_PLAN_SCHEMA_V10.into();
    input.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V10.into();
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun routed Branch test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V18.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V18.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V18.into();
    input.validate()?;
    Ok(input)
}
