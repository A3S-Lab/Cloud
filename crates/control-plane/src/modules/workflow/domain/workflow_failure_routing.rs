use super::{
    WorkflowDataType, WorkflowSpec, WorkflowStepFailureContract, WorkflowStepFallbackMode,
    WorkflowStepKind, WorkflowStepPort, WorkflowStepPortCardinality,
};
use std::collections::BTreeMap;

/// Validate the descriptor-bound failure routes in one already-validated DAG.
///
/// A failure route is not a second control-flow mechanism. It is the one named
/// outgoing edge of an Execution step whose handle exactly matches the error
/// output declared by that step's immutable descriptor failure contract.
pub(crate) fn validate_execution_failure_routes(
    workflow: &WorkflowSpec,
    failures: &BTreeMap<&str, &WorkflowStepFailureContract>,
) -> Result<bool, String> {
    let steps = workflow
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    let mut routed = false;
    for edge in &workflow.edges {
        let Some(handle) = edge.source_handle.as_deref() else {
            continue;
        };
        let source = steps.get(edge.source.as_str()).ok_or_else(|| {
            format!(
                "Workflow failure route {:?} lost source {:?}",
                edge.id, edge.source
            )
        })?;
        if source.kind == WorkflowStepKind::Branch {
            continue;
        }
        routed = true;
        if source.kind != WorkflowStepKind::Execution {
            return Err(format!(
                "Workflow failure route {:?} targets unsupported {} step {:?}",
                edge.id,
                source.kind.as_str(),
                source.id
            ));
        }
        let failure = failures.get(source.id.as_str()).ok_or_else(|| {
            format!(
                "Workflow Execution step {:?} has a failure route without immutable descriptor semantics",
                source.id
            )
        })?;
        let error_output = execution_failure_output(failure)?;
        if error_output.name != handle {
            return Err(format!(
                "Workflow Execution step {:?} failure handle {handle:?} does not match descriptor error output {:?}",
                source.id, error_output.name
            ));
        }
    }
    Ok(routed)
}

pub(crate) fn execution_failure_output(
    failure: &WorkflowStepFailureContract,
) -> Result<&WorkflowStepPort, String> {
    let output = failure.error_output.as_ref().ok_or_else(|| {
        "Workflow Execution failure route requires one typed descriptor error output".to_owned()
    })?;
    if failure.fallback != WorkflowStepFallbackMode::FailureBranch || !failure.failure_branch {
        return Err(
            "Workflow Execution failure route requires descriptor failure-branch fallback".into(),
        );
    }
    if output.cardinality != WorkflowStepPortCardinality::Single
        || !output.required
        || output.dynamic
        || !matches!(
            output.value_type,
            WorkflowDataType::Any | WorkflowDataType::Object
        )
    {
        return Err(
            "Workflow Execution failure output must be one required static object value".into(),
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use crate::modules::workflow::domain::{
        WorkflowEdgeSpec, WorkflowStepRetryClassification, WorkflowStepSpec,
    };

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn step(id: &str, kind: WorkflowStepKind) -> WorkflowStepSpec {
        WorkflowStepSpec {
            id: id.into(),
            label: id.into(),
            kind,
            configuration_digest: digest('a'),
            input_schema_digest: digest('b'),
            output_schema_digest: digest('c'),
            policy_digest: None,
            capability: None,
        }
    }

    fn failure(value_type: WorkflowDataType) -> WorkflowStepFailureContract {
        WorkflowStepFailureContract {
            error_output: Some(WorkflowStepPort {
                name: "error".into(),
                value_type,
                cardinality: WorkflowStepPortCardinality::Single,
                required: true,
                dynamic: false,
            }),
            retry_classification: WorkflowStepRetryClassification::OwnerClassified,
            fallback: WorkflowStepFallbackMode::FailureBranch,
            failure_branch: true,
        }
    }

    fn routed_workflow(kind: WorkflowStepKind) -> WorkflowSpec {
        WorkflowSpec {
            name: "Routed step".into(),
            description: String::new(),
            steps: vec![
                step("input", WorkflowStepKind::Input),
                step("run", kind),
                step("failure", WorkflowStepKind::Output),
                step("output", WorkflowStepKind::Output),
            ],
            edges: vec![
                WorkflowEdgeSpec {
                    id: "input-run".into(),
                    source: "input".into(),
                    target: "run".into(),
                    source_handle: None,
                },
                WorkflowEdgeSpec {
                    id: "run-failure".into(),
                    source: "run".into(),
                    target: "failure".into(),
                    source_handle: Some("error".into()),
                },
                WorkflowEdgeSpec {
                    id: "run-output".into(),
                    source: "run".into(),
                    target: "output".into(),
                    source_handle: None,
                },
            ],
        }
    }

    #[test]
    fn admits_only_execution_routes_with_static_object_failure_values() {
        let execution_failure = failure(WorkflowDataType::Object);
        let execution_failures = BTreeMap::from([("run", &execution_failure)]);
        assert_eq!(
            validate_execution_failure_routes(
                &routed_workflow(WorkflowStepKind::Execution),
                &execution_failures,
            ),
            Ok(true)
        );

        let unsupported = validate_execution_failure_routes(
            &routed_workflow(WorkflowStepKind::Transform),
            &execution_failures,
        )
        .expect_err("Transform failure routes remain gated");
        assert!(unsupported.contains("unsupported transform step"));

        let string_failure = failure(WorkflowDataType::String);
        let string_failures = BTreeMap::from([("run", &string_failure)]);
        let invalid = validate_execution_failure_routes(
            &routed_workflow(WorkflowStepKind::Execution),
            &string_failures,
        )
        .expect_err("failure values must be objects");
        assert!(invalid.contains("required static object value"));
    }
}
