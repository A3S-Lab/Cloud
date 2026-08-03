use super::super::types::{
    CompleteStepInput, CompleteStepOutput, FailStepInput, FailStepOutput, ValidateStepInput,
    ValidateStepOutput,
};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{bounded_reason, load_build, load_source};
use crate::modules::artifacts::domain::{
    BuildOutputValidationError, BuildRunFinalization, BuildRunStatus,
};
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
    if build.box_build_output.as_ref() != Some(input.output.as_ref()) {
        return Err(FlowError::Runtime(
            "build validation input changed the Box output receipt".into(),
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
    let source = load_source(runtime, &build).await?;
    let validated = match runtime
        .outputs
        .validate(&input.output, &source.recipe)
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
        .record_validated_output(validated.clone(), Utc::now().max(build.updated_at))
        .map_err(|error| flow_error("could not bind validated build output", error))?;
    runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist validated build output", error))?;
    Ok(ValidateStepOutput::Ready { output: validated })
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
    if build.cancellation_requested_at.is_some()
        && !(build.evidence_required
            && build.published_artifact.is_some()
            && build.evidence.is_none())
    {
        return Err(FlowError::Runtime(
            "cancelling build can fail only when required published evidence is missing".into(),
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
    let finalization = if build.status.is_terminal() {
        runtime
            .builds
            .finalize(build.clone(), build.aggregate_version)
            .await
            .map_err(|error| flow_error("could not verify build completion", error))?
    } else {
        let expected = build.aggregate_version;
        build
            .complete(input.cleaned_at.max(build.updated_at))
            .map_err(|error| flow_error("could not complete build run", error))?;
        runtime
            .builds
            .finalize(build, expected)
            .await
            .map_err(|error| flow_error("could not persist build completion", error))?
    };
    build = match finalization {
        BuildRunFinalization::Completed(build) => build,
        BuildRunFinalization::Rejected(mut rejected) => {
            let expected = rejected.aggregate_version;
            rejected
                .complete(input.cleaned_at.max(rejected.updated_at))
                .map_err(|error| flow_error("could not complete rejected hosted build", error))?;
            match runtime
                .builds
                .finalize(rejected, expected)
                .await
                .map_err(|error| flow_error("could not persist rejected hosted build", error))?
            {
                BuildRunFinalization::Completed(build) => build,
                BuildRunFinalization::Rejected(_) => {
                    return Err(FlowError::Runtime(
                        "hosted release rejection repeated after BuildRun failure".into(),
                    ))
                }
            }
        }
    };
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
