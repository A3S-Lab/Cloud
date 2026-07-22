use super::super::types::{
    PreparePublicationStepInput, PreparePublicationStepOutput, PublishStepInput, PublishStepOutput,
};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{bounded_reason, load_build};
use crate::modules::artifacts::domain::{
    BuildArtifactPublicationError, BuildRun, BuildRunStatus, OciPublicationRequest,
    PublishedOciArtifact,
};
use a3s_flow::FlowError;
use chrono::Utc;

pub(super) async fn prepare(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: PreparePublicationStepInput,
) -> a3s_flow::Result<PreparePublicationStepOutput> {
    let mut build = load_build(runtime, run_id, &input.flow).await?;
    if let Some(reason) = &build.failure {
        return Ok(PreparePublicationStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    if build.output.as_ref() != Some(&input.output) {
        return Err(FlowError::Runtime(
            "build publication input changed the validated OCI output".into(),
        ));
    }
    if let Some(target) = &build.publication_target {
        if !matches!(
            build.status,
            BuildRunStatus::Publishing | BuildRunStatus::Cancelling
        ) || !target.matches_output(&input.output)
        {
            return Err(FlowError::Runtime(
                "build publication target changed during replay".into(),
            ));
        }
        return Ok(PreparePublicationStepOutput::Ready {
            target: target.clone(),
            deadline_at: publication_deadline(runtime, &build)?,
        });
    }
    if build.cancellation_requested_at.is_some() {
        return Ok(PreparePublicationStepOutput::CancellationRequested);
    }
    if build.status != BuildRunStatus::Validating {
        return Err(FlowError::Runtime(format!(
            "build cannot prepare publication from {}",
            build.status.as_str()
        )));
    }
    let target = match runtime.publisher.target_for(&build) {
        Ok(target) => target,
        Err(error) => {
            return Ok(PreparePublicationStepOutput::Failed {
                reason: bounded_reason(error.to_string()),
            })
        }
    };
    let expected = build.aggregate_version;
    build
        .begin_publication(target.clone(), Utc::now().max(build.updated_at))
        .map_err(|error| flow_error("could not bind OCI publication target", error))?;
    let publishing = runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist OCI publication target", error))?;
    Ok(PreparePublicationStepOutput::Ready {
        target,
        deadline_at: publication_deadline(runtime, &publishing)?,
    })
}

pub(super) async fn publish(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: PublishStepInput,
) -> a3s_flow::Result<PublishStepOutput> {
    let build = load_build(runtime, run_id, &input.flow).await?;
    if let Some(reason) = &build.failure {
        return Ok(PublishStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    if build.output.as_ref() != Some(&input.output)
        || build.publication_target.as_ref() != Some(&input.target)
        || !matches!(
            build.status,
            BuildRunStatus::Publishing | BuildRunStatus::Cancelling
        )
    {
        return Err(FlowError::Runtime(
            "build publication changed its durable identity".into(),
        ));
    }
    if let Some(artifact) = &build.published_artifact {
        return publication_output(&build, artifact.clone());
    }
    let request = OciPublicationRequest::new(input.target, input.output)
        .map_err(|error| flow_error("could not reconstruct OCI publication", error))?;
    if build.cancellation_requested_at.is_some() {
        return match runtime.publisher.find(&request).await {
            Ok(Some(artifact)) => persist_publication(runtime, build, artifact).await,
            Ok(None) => Ok(PublishStepOutput::CancellationRequested { artifact: None }),
            Err(error) => Err(flow_error(
                "could not reconcile OCI publication after cancellation",
                error,
            )),
        };
    }
    if input.deadline_at < build.updated_at {
        return Err(FlowError::Runtime(
            "build publication deadline predates its durable target".into(),
        ));
    }
    if Utc::now() >= input.deadline_at {
        return match runtime.publisher.find(&request).await {
            Ok(Some(artifact)) => persist_publication(runtime, build, artifact).await,
            Ok(None) => Ok(PublishStepOutput::Failed {
                reason: "OCI publication exceeded its durable deadline".into(),
            }),
            Err(error) if terminal_error(&error) => Ok(PublishStepOutput::Failed {
                reason: bounded_reason(error.to_string()),
            }),
            Err(error) => Err(flow_error(
                "could not determine OCI publication outcome at its deadline",
                error,
            )),
        };
    }
    match runtime.publisher.publish(&request).await {
        Ok(artifact) => persist_publication(runtime, build, artifact).await,
        Err(error) if terminal_error(&error) => Ok(PublishStepOutput::Failed {
            reason: bounded_reason(error.to_string()),
        }),
        Err(error) => Err(flow_error("OCI publication is not ready", error)),
    }
}

async fn persist_publication(
    runtime: &BuildFlowRuntime,
    mut build: BuildRun,
    artifact: PublishedOciArtifact,
) -> a3s_flow::Result<PublishStepOutput> {
    let expected = build.aggregate_version;
    build
        .record_published_artifact(artifact.clone(), Utc::now().max(build.updated_at))
        .map_err(|error| flow_error("could not project published OCI artifact", error))?;
    let published = runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist published OCI artifact", error))?;
    publication_output(&published, artifact)
}

fn publication_output(
    build: &BuildRun,
    artifact: PublishedOciArtifact,
) -> a3s_flow::Result<PublishStepOutput> {
    if build.published_artifact.as_ref() != Some(&artifact) {
        return Err(FlowError::Runtime(
            "published OCI projection changed during replay".into(),
        ));
    }
    Ok(if build.cancellation_requested_at.is_some() {
        PublishStepOutput::CancellationRequested {
            artifact: Some(artifact),
        }
    } else {
        PublishStepOutput::Ready { artifact }
    })
}

fn publication_deadline(
    runtime: &BuildFlowRuntime,
    build: &BuildRun,
) -> a3s_flow::Result<chrono::DateTime<Utc>> {
    build
        .updated_at
        .checked_add_signed(runtime.config.publication_timeout)
        .ok_or_else(|| FlowError::Runtime("build publication deadline overflowed".into()))
}

fn terminal_error(error: &BuildArtifactPublicationError) -> bool {
    matches!(
        error,
        BuildArtifactPublicationError::Invalid(_)
            | BuildArtifactPublicationError::Integrity(_)
            | BuildArtifactPublicationError::Protocol(_)
            | BuildArtifactPublicationError::Registry(_)
    )
}
