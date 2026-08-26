use super::entities::{
    WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES, WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES,
};
use super::{WorkflowAgentStepOutput, WorkflowExecutionStepOutput};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, FormSubmissionId, HumanTaskId, OperationId,
    WorkflowDecisionId, WorkflowRunId,
};
use std::collections::BTreeSet;
use uuid::Uuid;

const EXECUTION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:executions:execution:";
const OPERATION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:operations:operation:";
const AGENT_CONVERSATION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:agents:conversation:";
const AGENT_EXECUTION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:agents:execution:";
const CONNECTOR_ATTEMPT_REFERENCE_PREFIX: &str = "urn:a3s:cloud:connectors:attempt:";
const FORM_SUBMISSION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:forms:submission:";
const HUMAN_TASK_REFERENCE_PREFIX: &str = "urn:a3s:cloud:workflow:human-task:";
const WORKFLOW_DECISION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:workflow:workflow-decision:";
const WORKFLOW_RUN_REFERENCE_PREFIX: &str = "urn:a3s:cloud:workflow:workflow-run:";
const WORKFLOW_COMPOSITE_CHILD_EVIDENCE_LIMIT: usize = WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES / 2;

pub(crate) fn execution_evidence_references(
    output: &WorkflowExecutionStepOutput,
) -> Result<Vec<String>, String> {
    output.validate_shape()?;
    checked_evidence_references([
        format!("{EXECUTION_REFERENCE_PREFIX}{}", output.execution_id),
        format!("{OPERATION_REFERENCE_PREFIX}{}", output.operation_id),
    ])
}

pub(crate) fn agent_evidence_references(
    output: &WorkflowAgentStepOutput,
) -> Result<Vec<String>, String> {
    output.validate_shape()?;
    agent_identity_evidence_references(
        output.conversation_id,
        output.agent_execution_id,
        output.operation_id,
    )
}

pub(crate) fn agent_identity_evidence_references(
    conversation_id: AgentConversationId,
    execution_id: AgentExecutionId,
    operation_id: OperationId,
) -> Result<Vec<String>, String> {
    checked_evidence_references([
        format!("{AGENT_CONVERSATION_REFERENCE_PREFIX}{conversation_id}"),
        format!("{AGENT_EXECUTION_REFERENCE_PREFIX}{execution_id}"),
        format!("{OPERATION_REFERENCE_PREFIX}{operation_id}"),
    ])
}

pub(crate) fn connector_attempt_evidence_references(
    attempt_ids: impl IntoIterator<Item = Uuid>,
) -> Result<Vec<String>, String> {
    checked_evidence_references(
        attempt_ids
            .into_iter()
            .map(|attempt_id| format!("{CONNECTOR_ATTEMPT_REFERENCE_PREFIX}{attempt_id}")),
    )
}

pub(crate) fn human_decision_evidence_references(
    human_task_id: HumanTaskId,
    workflow_decision_id: WorkflowDecisionId,
    form_submission_id: Option<FormSubmissionId>,
) -> Result<Vec<String>, String> {
    checked_evidence_references(
        [
            Some(format!("{HUMAN_TASK_REFERENCE_PREFIX}{human_task_id}")),
            Some(format!(
                "{WORKFLOW_DECISION_REFERENCE_PREFIX}{workflow_decision_id}"
            )),
            form_submission_id
                .map(|submission_id| format!("{FORM_SUBMISSION_REFERENCE_PREFIX}{submission_id}")),
        ]
        .into_iter()
        .flatten(),
    )
}

pub(crate) fn composite_child_evidence_references(
    child_workflow_run_ids: impl IntoIterator<Item = WorkflowRunId>,
) -> Result<Vec<String>, String> {
    let child_workflow_run_ids = child_workflow_run_ids.into_iter().collect::<Vec<_>>();
    let retained_from = child_workflow_run_ids
        .len()
        .saturating_sub(WORKFLOW_COMPOSITE_CHILD_EVIDENCE_LIMIT);
    checked_evidence_references(
        child_workflow_run_ids
            .into_iter()
            .skip(retained_from)
            .flat_map(|workflow_run_id| {
                [
                    format!("{OPERATION_REFERENCE_PREFIX}{workflow_run_id}"),
                    format!("{WORKFLOW_RUN_REFERENCE_PREFIX}{workflow_run_id}"),
                ]
            }),
    )
}

pub(crate) fn validate_evidence_references(references: &[String]) -> Result<(), String> {
    if references.len() > WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES {
        return Err("Workflow step evidence reference count exceeds its bound".into());
    }
    if references
        .windows(2)
        .any(|pair| pair[0].as_str() >= pair[1].as_str())
    {
        return Err("Workflow step evidence references must be unique and sorted".into());
    }
    for reference in references {
        if reference.is_empty()
            || reference.len() > WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES
            || reference.contains(['\0', '\r', '\n'])
            || !valid_reference(reference)
        {
            return Err("Workflow step evidence reference is invalid".into());
        }
    }
    Ok(())
}

fn checked_evidence_references(
    references: impl IntoIterator<Item = String>,
) -> Result<Vec<String>, String> {
    let references = references.into_iter().collect::<BTreeSet<_>>();
    let references = references.into_iter().collect::<Vec<_>>();
    validate_evidence_references(&references)?;
    Ok(references)
}

fn valid_reference(reference: &str) -> bool {
    [
        EXECUTION_REFERENCE_PREFIX,
        AGENT_CONVERSATION_REFERENCE_PREFIX,
        AGENT_EXECUTION_REFERENCE_PREFIX,
        OPERATION_REFERENCE_PREFIX,
        CONNECTOR_ATTEMPT_REFERENCE_PREFIX,
        FORM_SUBMISSION_REFERENCE_PREFIX,
        HUMAN_TASK_REFERENCE_PREFIX,
        WORKFLOW_DECISION_REFERENCE_PREFIX,
        WORKFLOW_RUN_REFERENCE_PREFIX,
    ]
    .into_iter()
    .find_map(|prefix| reference.strip_prefix(prefix))
    .and_then(|identity| Uuid::parse_str(identity).ok())
    .is_some_and(|identity| !identity.is_nil())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, AgentConversationId, AgentExecutionId, AssetId, AssetReleaseId,
        ExecutionId, ExecutionTemplateId, ExecutionTemplateRevisionId, OperationId, Sha256Digest,
    };
    use crate::modules::workflow::domain::{
        WorkflowAgentOutcome, WorkflowAgentStepOutput, WorkflowExecutionOutcome,
        WORKFLOW_AGENT_RESULT_SCHEMA, WORKFLOW_EXECUTION_RESULT_SCHEMA,
    };
    use chrono::Utc;

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    #[test]
    fn execution_references_retain_child_and_operation_authority() {
        let execution_id = ExecutionId::new();
        let output = WorkflowExecutionStepOutput {
            schema: WORKFLOW_EXECUTION_RESULT_SCHEMA.into(),
            execution_id,
            operation_id: OperationId::from_uuid(execution_id.as_uuid()),
            execution_template_id: ExecutionTemplateId::new(),
            execution_template_revision_id: ExecutionTemplateRevisionId::new(),
            execution_template_digest: digest('a'),
            invocation_template_digest: digest('b'),
            outcome: WorkflowExecutionOutcome::Succeeded { exit_code: 0 },
            finished_at: canonical_timestamp(Utc::now()),
        };

        assert_eq!(
            execution_evidence_references(&output).expect("Execution evidence references"),
            [
                format!("{EXECUTION_REFERENCE_PREFIX}{execution_id}"),
                format!("{OPERATION_REFERENCE_PREFIX}{execution_id}"),
            ]
        );
    }

    #[test]
    fn agent_references_retain_conversation_execution_and_operation_authority() {
        let conversation_id = AgentConversationId::new();
        let execution_id = AgentExecutionId::new();
        let operation_id = OperationId::new();
        let output = WorkflowAgentStepOutput {
            schema: WORKFLOW_AGENT_RESULT_SCHEMA.into(),
            conversation_id,
            agent_execution_id: execution_id,
            operation_id,
            agent_asset_id: AssetId::new(),
            agent_asset_release_id: AssetReleaseId::new(),
            agent_release_digest: digest('a'),
            provider: None,
            outcome: WorkflowAgentOutcome::Cancelled,
            text: String::new(),
            terminal_event_sequence: 2,
            finished_at: canonical_timestamp(Utc::now()),
        };

        assert_eq!(
            agent_evidence_references(&output).expect("Agent evidence references"),
            [
                format!("{AGENT_CONVERSATION_REFERENCE_PREFIX}{conversation_id}"),
                format!("{AGENT_EXECUTION_REFERENCE_PREFIX}{execution_id}"),
                format!("{OPERATION_REFERENCE_PREFIX}{operation_id}"),
            ]
        );
    }

    #[test]
    fn connector_attempt_references_are_deduplicated_and_sorted() {
        let first = Uuid::parse_str("019c0000-0000-7000-8000-000000000001").expect("first");
        let second = Uuid::parse_str("019c0000-0000-7000-8000-000000000002").expect("second");

        assert_eq!(
            connector_attempt_evidence_references([second, first, second])
                .expect("Connector evidence references"),
            [
                format!("{CONNECTOR_ATTEMPT_REFERENCE_PREFIX}{first}"),
                format!("{CONNECTOR_ATTEMPT_REFERENCE_PREFIX}{second}"),
            ]
        );
    }

    #[test]
    fn human_decision_references_retain_task_decision_and_optional_submission_authority() {
        let human_task_id = HumanTaskId::from_uuid(
            Uuid::parse_str("019c0000-0000-7000-8000-000000000003").expect("HumanTask"),
        );
        let workflow_decision_id = WorkflowDecisionId::from_uuid(
            Uuid::parse_str("019c0000-0000-7000-8000-000000000002").expect("WorkflowDecision"),
        );
        let form_submission_id = FormSubmissionId::from_uuid(
            Uuid::parse_str("019c0000-0000-7000-8000-000000000001").expect("FormSubmission"),
        );

        assert_eq!(
            human_decision_evidence_references(
                human_task_id,
                workflow_decision_id,
                Some(form_submission_id),
            )
            .expect("interactive HumanDecision evidence references"),
            [
                format!("{FORM_SUBMISSION_REFERENCE_PREFIX}{form_submission_id}"),
                format!("{HUMAN_TASK_REFERENCE_PREFIX}{human_task_id}"),
                format!("{WORKFLOW_DECISION_REFERENCE_PREFIX}{workflow_decision_id}"),
            ]
        );
        assert_eq!(
            human_decision_evidence_references(human_task_id, workflow_decision_id, None)
                .expect("automatic HumanDecision evidence references"),
            [
                format!("{HUMAN_TASK_REFERENCE_PREFIX}{human_task_id}"),
                format!("{WORKFLOW_DECISION_REFERENCE_PREFIX}{workflow_decision_id}"),
            ]
        );
    }

    #[test]
    fn composite_child_references_retain_the_latest_bounded_frame_window() {
        let children = (1_u128..=18)
            .map(Uuid::from_u128)
            .map(WorkflowRunId::from_uuid)
            .collect::<Vec<_>>();

        let references = composite_child_evidence_references(children.clone())
            .expect("composite child evidence references");

        assert_eq!(references.len(), WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES);
        for child in &children[..2] {
            assert!(!references
                .iter()
                .any(|reference| reference.ends_with(&child.to_string())));
        }
        for child in &children[2..] {
            assert!(references.contains(&format!("{OPERATION_REFERENCE_PREFIX}{child}")));
            assert!(references.contains(&format!("{WORKFLOW_RUN_REFERENCE_PREFIX}{child}")));
        }
    }

    #[test]
    fn evidence_reference_contract_is_closed_and_canonical() {
        let id = Uuid::now_v7();
        let valid = format!("{CONNECTOR_ATTEMPT_REFERENCE_PREFIX}{id}");
        validate_evidence_references(std::slice::from_ref(&valid)).expect("valid reference");

        assert!(validate_evidence_references(&[valid.clone(), valid]).is_err());
        assert!(validate_evidence_references(&[
            format!("{OPERATION_REFERENCE_PREFIX}{id}"),
            format!("{EXECUTION_REFERENCE_PREFIX}{id}"),
        ])
        .is_err());
        assert!(
            validate_evidence_references(&[format!("urn:a3s:cloud:workflow:unknown:{id}")])
                .is_err()
        );
        assert!(validate_evidence_references(&[format!(
            "{EXECUTION_REFERENCE_PREFIX}{}",
            Uuid::nil()
        )])
        .is_err());
    }
}
