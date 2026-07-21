use super::super::types::{BuildFlowInput, PrepareStepOutput, PreparedBuild};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::bounded_reason;
use crate::modules::artifacts::domain::{BuildInputPreparationError, BuildRunStatus};
use crate::modules::shared_kernel::domain::RepositoryError;
use chrono::Utc;

pub(super) async fn prepare(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: BuildFlowInput,
) -> a3s_flow::Result<PrepareStepOutput> {
    let mut build = match runtime
        .builds
        .find(input.organization_id, input.build_run_id)
        .await
    {
        Ok(build) => build,
        Err(RepositoryError::NotFound) => {
            return Ok(PrepareStepOutput::Rejected {
                reason: "build operation ownership validation failed".into(),
            })
        }
        Err(error) => return Err(flow_error("could not load build run", error)),
    };
    if build.id != input.build_run_id
        || build.organization_id != input.organization_id
        || build.operation_id.to_string() != run_id
        || build.operation_id.as_uuid() != build.id.as_uuid()
    {
        return Ok(PrepareStepOutput::Rejected {
            reason: "build operation ownership validation failed".into(),
        });
    }
    let revision = match runtime
        .sources
        .find(build.organization_id, build.source_revision_id)
        .await
    {
        Ok(revision) => revision,
        Err(RepositoryError::NotFound) => {
            return Ok(PrepareStepOutput::Failed {
                reason: "build source revision is unavailable".into(),
            })
        }
        Err(error) => return Err(flow_error("could not load build source revision", error)),
    };
    if revision.organization_id != build.organization_id
        || revision.project_id != build.project_id
        || revision.environment_id != build.environment_id
        || revision.id != build.source_revision_id
    {
        return Ok(PrepareStepOutput::Failed {
            reason: "build source revision does not match persisted build ownership".into(),
        });
    }
    let deadline = build
        .requested_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            a3s_flow::FlowError::Runtime("build preparation deadline overflowed".into())
        })?;
    if build.cancellation_requested_at.is_some() {
        return Ok(PrepareStepOutput::CancellationRequested);
    }
    if build.failure.is_some() {
        return Ok(PrepareStepOutput::Failed {
            reason: build
                .failure
                .clone()
                .unwrap_or_else(|| "build failed before input preparation".into()),
        });
    }
    if build.status == BuildRunStatus::Queued {
        let expected = build.aggregate_version;
        build
            .begin_preparation(Utc::now().max(build.updated_at))
            .map_err(|error| flow_error("could not begin build preparation", error))?;
        build = runtime
            .builds
            .save(build, expected)
            .await
            .map_err(|error| flow_error("could not persist build preparation", error))?;
    }
    if build.status == BuildRunStatus::Preparing {
        let prepared = match runtime.inputs.prepare(&build, &revision).await {
            Ok(prepared) => prepared,
            Err(
                error @ (BuildInputPreparationError::Unavailable(_)
                | BuildInputPreparationError::Storage(_)),
            ) if Utc::now() < deadline => {
                return Err(flow_error("build input is not ready", error))
            }
            Err(error) => {
                return Ok(PrepareStepOutput::Failed {
                    reason: bounded_reason(error.to_string()),
                })
            }
        };
        let expected = build.aggregate_version;
        build
            .record_input(
                prepared.source_content_digest,
                prepared.artifact,
                Utc::now().max(build.updated_at),
            )
            .map_err(|error| flow_error("could not bind prepared build input", error))?;
        build = runtime
            .builds
            .save(build, expected)
            .await
            .map_err(|error| flow_error("could not persist prepared build input", error))?;
    }
    if !matches!(
        build.status,
        BuildRunStatus::Prepared
            | BuildRunStatus::Scheduled
            | BuildRunStatus::Running
            | BuildRunStatus::Validating
            | BuildRunStatus::CleanupPending
            | BuildRunStatus::Succeeded
    ) {
        return Err(a3s_flow::FlowError::Runtime(format!(
            "build cannot prepare input from {}",
            build.status.as_str()
        )));
    }
    Ok(PrepareStepOutput::Ready {
        prepared: Box::new(PreparedBuild {
            organization_id: build.organization_id,
            build_run_id: build.id,
            source_revision_id: build.source_revision_id,
            source_content_digest: build.source_content_digest.clone().ok_or_else(|| {
                a3s_flow::FlowError::Runtime("prepared build omitted its source digest".into())
            })?,
            input_artifact: build.input_artifact.clone().ok_or_else(|| {
                a3s_flow::FlowError::Runtime("prepared build omitted its input Artifact".into())
            })?,
            recipe: revision.recipe,
            convergence_deadline: deadline,
        }),
    })
}
