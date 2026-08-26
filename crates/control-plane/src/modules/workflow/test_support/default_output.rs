use super::*;

pub(crate) fn default_output_execution_workflow_run_input() -> Result<WorkflowRunInput, String> {
    let mut input = routed_execution_workflow_run_input()?;
    input.plan.steps.retain(|step| step.id != "failure_output");
    input.plan.edges.retain(|edge| edge.id != "execute-failure");

    let default_output = WorkflowDefaultOutput::new(
        "result",
        serde_json::json!({"status": "temporarily_unavailable"}),
    )?;
    let default_policy =
        WorkflowPayload::from_content(WorkflowPayloadContent::Policy(WorkflowPolicy {
            mode: WorkflowPolicyMode::Static,
            expression: None,
            candidates: Vec::new(),
            retry: None,
            default_output: Some(default_output),
            cancellation_compensation: None,
        }))?;
    let policy_digest = default_policy.digest().clone();
    input
        .payloads
        .push(ResolvedWorkflowPayload::from_payload(&default_policy));
    input
        .payloads
        .sort_by(|left, right| left.digest.cmp(&right.digest));

    for step in &mut input.plan.steps {
        step.default_output = None;
        if step.id == TEST_EXECUTION_STEP_ID {
            step.policy_digest = Some(policy_digest.clone());
            step.failure = Some(WorkflowStepFailureContract {
                error_output: None,
                retry_classification: WorkflowStepRetryClassification::OwnerClassified,
                fallback: WorkflowStepFallbackMode::DefaultOutput,
                failure_branch: false,
            });
            step.default_output = Some(WorkflowStepDefaultOutputContract {
                output_port: WorkflowStepPort {
                    name: "result".into(),
                    value_type: WorkflowDataType::Any,
                    cardinality: WorkflowStepPortCardinality::Single,
                    required: true,
                    dynamic: false,
                },
            });
        }
    }
    let restored_payloads = input
        .payloads
        .iter()
        .map(ResolvedWorkflowPayload::restore)
        .collect::<Result<Vec<_>, _>>()?;
    input.plan.workflow_payload_set_digest = digest_payload_set(&restored_payloads)?;
    input.plan.schema = WORKFLOW_PLAN_SCHEMA_V4.into();
    input.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V4.into();
    input.plan.validate()?;
    input.plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun default-output execution test plan",
    )?))?;
    input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V7.into();
    input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7.into();
    input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V7.into();
    input.validate()?;
    Ok(input)
}
