use super::super::types::{
    CompleteStepInput, CompleteStepOutput, FailStepInput, FailStepOutput, ValidateStepInput,
    ValidateStepOutput,
};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{bounded_reason, load_build, load_revision};
use crate::modules::artifacts::domain::{BuildOutputValidationError, BuildRunStatus};
use a3s_flow::FlowError;
use chrono::Utc;

pub(super) async fn validate(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: ValidateStepInput,
) -> a3s_flow::Result<ValidateStepOutput> {
    let mut build = load_build(runtime, run_id, &input.flow).await?;
    if build.cancellation_requested_at.is_some() {
        return Ok(ValidateStepOutput::CancellationRequested);
    }
    if let Some(reason) = &build.failure {
        return Ok(ValidateStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    if build.runtime_output_artifact.as_ref() != Some(&input.artifact) {
        return Err(FlowError::Runtime(
            "build validation input changed the Runtime output Artifact".into(),
        ));
    }
    if let Some(output) = &build.output {
        return Ok(ValidateStepOutput::Ready {
            output: output.clone(),
        });
    }
    if build.status != BuildRunStatus::Validating {
        return Err(FlowError::Runtime(format!(
            "build cannot validate output from {}",
            build.status.as_str()
        )));
    }
    let revision = load_revision(runtime, &build).await?;
    let output = match runtime
        .outputs
        .validate(&input.artifact, &revision.recipe)
        .await
    {
        Ok(output) => output,
        Err(
            error @ (BuildOutputValidationError::Unavailable(_)
            | BuildOutputValidationError::Storage(_)),
        ) => {
            return Err(flow_error(
                "build output is not ready for validation",
                error,
            ))
        }
        Err(error) => {
            return Ok(ValidateStepOutput::Failed {
                reason: bounded_reason(error.to_string()),
            })
        }
    };
    let expected = build.aggregate_version;
    build
        .record_validated_output(output.clone(), Utc::now().max(build.updated_at))
        .map_err(|error| flow_error("could not bind validated build output", error))?;
    runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist validated build output", error))?;
    Ok(ValidateStepOutput::Ready { output })
}

pub(super) async fn fail(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: FailStepInput,
) -> a3s_flow::Result<FailStepOutput> {
    let mut build = load_build(runtime, run_id, &input.flow).await?;
    let reason = bounded_reason(input.reason);
    if build.status.is_terminal() {
        return Ok(FailStepOutput {
            reason: build.failure.unwrap_or(reason),
            failed_at: build.finished_at.unwrap_or(build.updated_at),
        });
    }
    if build.cancellation_requested_at.is_some() {
        return Err(FlowError::Runtime(
            "cancelling build cannot record a failure".into(),
        ));
    }
    if let Some(existing) = &build.failure {
        if existing != &reason {
            return Err(FlowError::Runtime(
                "build failure reason changed during replay".into(),
            ));
        }
        return Ok(FailStepOutput {
            reason,
            failed_at: build.updated_at,
        });
    }
    let expected = build.aggregate_version;
    build
        .record_failure(reason.clone(), Utc::now().max(build.updated_at))
        .map_err(|error| flow_error("could not record build failure", error))?;
    let failed = runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist build failure", error))?;
    Ok(FailStepOutput {
        reason,
        failed_at: failed.updated_at,
    })
}

pub(super) async fn complete(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: CompleteStepInput,
) -> a3s_flow::Result<CompleteStepOutput> {
    let mut build = load_build(runtime, run_id, &input.flow).await?;
    runtime
        .inputs
        .remove(&build)
        .await
        .map_err(|error| flow_error("could not remove materialized build input", error))?;
    if !build.status.is_terminal() {
        let expected = build.aggregate_version;
        build
            .complete(input.cleaned_at.max(build.updated_at))
            .map_err(|error| flow_error("could not complete build run", error))?;
        build = runtime
            .builds
            .save(build, expected)
            .await
            .map_err(|error| flow_error("could not persist build completion", error))?;
    }
    Ok(CompleteStepOutput {
        build_run_id: build.id,
        status: build.status,
        output: build.output,
        published_artifact: build.published_artifact,
        failure: build.failure,
        finished_at: build
            .finished_at
            .ok_or_else(|| FlowError::Runtime("terminal build omitted its finish time".into()))?,
    })
}
