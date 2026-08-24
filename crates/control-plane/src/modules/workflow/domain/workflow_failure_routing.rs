use super::{
    CapabilityType, WorkflowDataType, WorkflowSpec, WorkflowStepFailureContract,
    WorkflowStepFallbackMode, WorkflowStepKind, WorkflowStepPort, WorkflowStepPortCardinality,
    WorkflowStepSpec,
};
use std::collections::{BTreeMap, BTreeSet};

/// Validate the descriptor-bound failure routes in one already-validated DAG.
///
/// A failure route is not a second control-flow mechanism. It is the one named
/// outgoing edge of a runtime-supported step whose handle exactly matches the
/// error output declared by that step's immutable descriptor failure contract.
pub(crate) fn validate_descriptor_failure_routes(
    workflow: &WorkflowSpec,
    failures: &BTreeMap<&str, &WorkflowStepFailureContract>,
    application_variable_steps: &BTreeSet<&str>,
    application_answer_steps: &BTreeSet<&str>,
    workflow_output_steps: &BTreeSet<&str>,
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
            let Some(failure) = failures.get(source.id.as_str()) else {
                continue;
            };
            if failure
                .error_output
                .as_ref()
                .is_none_or(|output| output.name != handle)
            {
                continue;
            }
        }
        routed = true;
        if !supports_failure_route(
            source,
            application_variable_steps,
            application_answer_steps,
            workflow_output_steps,
        ) {
            return Err(format!(
                "Workflow failure route {:?} targets unsupported {} step {:?}",
                edge.id,
                source.kind.as_str(),
                source.id
            ));
        }
        let failure = failures.get(source.id.as_str()).ok_or_else(|| {
            format!(
                "Workflow step {:?} has a failure route without immutable descriptor semantics",
                source.id
            )
        })?;
        let error_output = descriptor_failure_output(failure)?;
        if error_output.name != handle {
            return Err(format!(
                "Workflow step {:?} failure handle {handle:?} does not match descriptor error output {:?}",
                source.id, error_output.name
            ));
        }
    }
    Ok(routed)
}

pub(crate) fn has_application_variable_failure_route(
    workflow: &WorkflowSpec,
    application_variable_steps: &BTreeSet<&str>,
) -> bool {
    workflow.edges.iter().any(|edge| {
        edge.source_handle.is_some() && application_variable_steps.contains(edge.source.as_str())
    })
}

pub(crate) fn has_application_answer_failure_route(
    workflow: &WorkflowSpec,
    application_answer_steps: &BTreeSet<&str>,
) -> bool {
    workflow.edges.iter().any(|edge| {
        edge.source_handle.is_some() && application_answer_steps.contains(edge.source.as_str())
    })
}

pub(crate) fn has_connector_failure_route(workflow: &WorkflowSpec) -> bool {
    let steps = workflow
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    workflow.edges.iter().any(|edge| {
        edge.source_handle.is_some()
            && steps
                .get(edge.source.as_str())
                .is_some_and(|step| is_connector_step(step))
    })
}

pub(crate) fn has_transform_failure_route(workflow: &WorkflowSpec) -> bool {
    let steps = workflow
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    workflow.edges.iter().any(|edge| {
        edge.source_handle.is_some()
            && steps
                .get(edge.source.as_str())
                .is_some_and(|step| step.kind == WorkflowStepKind::Transform)
    })
}

pub(crate) fn has_branch_failure_route(
    workflow: &WorkflowSpec,
    failures: &BTreeMap<&str, &WorkflowStepFailureContract>,
) -> bool {
    let steps = workflow
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    workflow.edges.iter().any(|edge| {
        let Some(handle) = edge.source_handle.as_deref() else {
            return false;
        };
        steps
            .get(edge.source.as_str())
            .is_some_and(|step| step.kind == WorkflowStepKind::Branch)
            && failures
                .get(edge.source.as_str())
                .and_then(|failure| failure.error_output.as_ref())
                .is_some_and(|output| output.name == handle)
    })
}

pub(crate) fn has_workflow_output_failure_route(
    workflow: &WorkflowSpec,
    workflow_output_steps: &BTreeSet<&str>,
) -> bool {
    workflow.edges.iter().any(|edge| {
        edge.source_handle.is_some() && workflow_output_steps.contains(edge.source.as_str())
    })
}

pub(crate) fn descriptor_failure_output(
    failure: &WorkflowStepFailureContract,
) -> Result<&WorkflowStepPort, String> {
    let output = failure.error_output.as_ref().ok_or_else(|| {
        "Workflow failure route requires one typed descriptor error output".to_owned()
    })?;
    if failure.fallback != WorkflowStepFallbackMode::FailureBranch || !failure.failure_branch {
        return Err("Workflow failure route requires descriptor failure-branch fallback".into());
    }
    if output.cardinality != WorkflowStepPortCardinality::Single
        || !output.required
        || output.dynamic
        || !matches!(
            output.value_type,
            WorkflowDataType::Any | WorkflowDataType::Object
        )
    {
        return Err("Workflow failure output must be one required static object value".into());
    }
    Ok(output)
}

fn supports_failure_route(
    step: &WorkflowStepSpec,
    application_variable_steps: &BTreeSet<&str>,
    application_answer_steps: &BTreeSet<&str>,
    workflow_output_steps: &BTreeSet<&str>,
) -> bool {
    step.kind == WorkflowStepKind::Transform
        || step.kind == WorkflowStepKind::Branch
        || step.kind == WorkflowStepKind::Execution
        || is_connector_step(step)
        || (step.kind == WorkflowStepKind::Service
            && step.capability.is_none()
            && application_variable_steps.contains(step.id.as_str()))
        || (step.kind == WorkflowStepKind::Output
            && step.capability.is_none()
            && (application_answer_steps.contains(step.id.as_str())
                || workflow_output_steps.contains(step.id.as_str())))
}

fn is_connector_step(step: &WorkflowStepSpec) -> bool {
    step.kind == WorkflowStepKind::Service
        && step.capability.as_ref().is_some_and(|capability| {
            capability.capability_type == CapabilityType::ConnectorRevision
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use crate::modules::workflow::domain::{
        CapabilityOwner, CapabilityReference, CapabilityType, WorkflowContract, WorkflowEdgeSpec,
        WorkflowStepRetryClassification, WorkflowStepSpec,
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

    fn routed_connector_workflow() -> WorkflowSpec {
        let mut workflow = routed_workflow(WorkflowStepKind::Service);
        workflow.steps[1].capability = Some(CapabilityReference {
            owner: CapabilityOwner::Connectors,
            capability_type: CapabilityType::ConnectorRevision,
            resource_id: uuid::Uuid::new_v4(),
            revision: uuid::Uuid::new_v4().to_string(),
            digest: digest('d'),
            capability: "connector.http".into(),
        });
        workflow
    }

    #[test]
    fn admits_only_runtime_supported_routes_with_static_object_failure_values() {
        let execution_failure = failure(WorkflowDataType::Object);
        let execution_failures = BTreeMap::from([("run", &execution_failure)]);
        let no_application_steps = BTreeSet::new();
        let no_workflow_output_steps = BTreeSet::new();
        assert_eq!(
            validate_descriptor_failure_routes(
                &routed_workflow(WorkflowStepKind::Execution),
                &execution_failures,
                &no_application_steps,
                &no_application_steps,
                &no_workflow_output_steps,
            ),
            Ok(true)
        );

        assert_eq!(
            validate_descriptor_failure_routes(
                &routed_connector_workflow(),
                &execution_failures,
                &no_application_steps,
                &no_application_steps,
                &no_workflow_output_steps,
            ),
            Ok(true)
        );

        let transform_workflow = routed_workflow(WorkflowStepKind::Transform);
        assert_eq!(
            validate_descriptor_failure_routes(
                &transform_workflow,
                &execution_failures,
                &no_application_steps,
                &no_application_steps,
                &no_workflow_output_steps,
            ),
            Ok(true)
        );
        assert!(has_transform_failure_route(&transform_workflow));

        let unbound_service = validate_descriptor_failure_routes(
            &routed_workflow(WorkflowStepKind::Service),
            &execution_failures,
            &no_application_steps,
            &no_application_steps,
            &no_workflow_output_steps,
        )
        .expect_err("unbound Service failure routes remain gated");
        assert!(unbound_service.contains("unsupported service step"));

        assert_eq!(
            validate_descriptor_failure_routes(
                &routed_workflow(WorkflowStepKind::Service),
                &execution_failures,
                &BTreeSet::from(["run"]),
                &no_application_steps,
                &no_workflow_output_steps,
            ),
            Ok(true)
        );

        let answer_workflow = routed_workflow(WorkflowStepKind::Output);
        WorkflowContract::from_spec(answer_workflow.clone())
            .expect("candidate handled Output route");
        assert_eq!(
            validate_descriptor_failure_routes(
                &answer_workflow,
                &execution_failures,
                &no_application_steps,
                &BTreeSet::from(["run"]),
                &no_workflow_output_steps,
            ),
            Ok(true)
        );
        assert_eq!(
            validate_descriptor_failure_routes(
                &answer_workflow,
                &execution_failures,
                &no_application_steps,
                &no_application_steps,
                &BTreeSet::from(["run"]),
            ),
            Ok(true)
        );
        assert!(has_workflow_output_failure_route(
            &answer_workflow,
            &BTreeSet::from(["run"]),
        ));
        let unbound_output = validate_descriptor_failure_routes(
            &answer_workflow,
            &execution_failures,
            &no_application_steps,
            &no_application_steps,
            &no_workflow_output_steps,
        )
        .expect_err("ordinary Output failure routes remain gated");
        assert!(unbound_output.contains("unsupported output step"));

        let string_failure = failure(WorkflowDataType::String);
        let string_failures = BTreeMap::from([("run", &string_failure)]);
        let invalid = validate_descriptor_failure_routes(
            &routed_workflow(WorkflowStepKind::Execution),
            &string_failures,
            &no_application_steps,
            &no_application_steps,
            &no_workflow_output_steps,
        )
        .expect_err("failure values must be objects");
        assert!(invalid.contains("required static object value"));
    }

    #[test]
    fn branch_routes_distinguish_descriptor_failure_from_business_handles() {
        let branch_failure = failure(WorkflowDataType::Object);
        let failures = BTreeMap::from([("run", &branch_failure)]);
        let no_application_steps = BTreeSet::new();
        let no_workflow_output_steps = BTreeSet::new();
        let mut workflow = routed_workflow(WorkflowStepKind::Branch);
        workflow.edges.push(WorkflowEdgeSpec {
            id: "run-matched".into(),
            source: "run".into(),
            target: "output".into(),
            source_handle: Some("matched".into()),
        });

        assert_eq!(
            validate_descriptor_failure_routes(
                &workflow,
                &failures,
                &no_application_steps,
                &no_application_steps,
                &no_workflow_output_steps,
            ),
            Ok(true)
        );
    }
}
