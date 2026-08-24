use super::entities::{
    WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES, WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES,
};
use super::WorkflowExecutionStepOutput;
use std::collections::BTreeSet;
use uuid::Uuid;

const EXECUTION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:executions:execution:";
const OPERATION_REFERENCE_PREFIX: &str = "urn:a3s:cloud:operations:operation:";
const CONNECTOR_ATTEMPT_REFERENCE_PREFIX: &str = "urn:a3s:cloud:connectors:attempt:";

pub(crate) fn execution_evidence_references(
    output: &WorkflowExecutionStepOutput,
) -> Result<Vec<String>, String> {
    output.validate_shape()?;
    checked_evidence_references([
        format!("{EXECUTION_REFERENCE_PREFIX}{}", output.execution_id),
        format!("{OPERATION_REFERENCE_PREFIX}{}", output.operation_id),
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
        OPERATION_REFERENCE_PREFIX,
        CONNECTOR_ATTEMPT_REFERENCE_PREFIX,
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
        canonical_timestamp, ExecutionId, ExecutionTemplateId, ExecutionTemplateRevisionId,
        OperationId, Sha256Digest,
    };
    use crate::modules::workflow::domain::{
        WorkflowExecutionOutcome, WORKFLOW_EXECUTION_RESULT_SCHEMA,
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
