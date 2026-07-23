use super::flow::acknowledge_removal;
use super::support::*;
use crate::modules::artifacts::domain::{
    BuildEvidenceGenerationError, BuildRunStatus, IBuildRunRepository,
};
use crate::modules::artifacts::infrastructure::build_flow::types::{
    AttestStepInput, AttestStepOutput, BuildFlowInput, PreparePublicationStepInput,
    PreparePublicationStepOutput, PrepareStepOutput, PublishStepInput, PublishStepOutput,
};
use a3s_cloud_contracts::NodeCommandPayload;
use a3s_flow::{FlowEngine, FlowError, FlowRuntime, StepInvocation, WorkflowRunStatus};
use chrono::{Duration, Utc};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;

#[tokio::test]
async fn lost_attestation_completion_replays_durable_prefix_without_repeating_side_effects(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let store = Arc::new(FailOnceStepCompletionStore::new("attest"));
    let engine = FlowEngine::new(store.clone(), Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    let apply_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        0,
    )
    .await?;
    let apply = apply_lease.commands.first().ok_or("missing build apply")?;
    let NodeCommandPayload::RuntimeApply { request } = &apply.payload else {
        return Err("build command is not Runtime apply".into());
    };
    record_observation(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        &fixture.capabilities,
        apply,
        succeeded_observation(&request.spec, fixture.outputs.artifact())?,
    )
    .await?;

    let failure = engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await
        .expect_err("injected crash must lose the attest StepCompleted event");
    assert!(matches!(failure, FlowError::Store(_)));
    let attesting = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(attesting.status, BuildRunStatus::Attesting);
    assert!(attesting.evidence.is_some());
    assert!(attesting.cleanup_command_id.is_none());
    assert_eq!(fixture.publisher.publications(), 1);
    assert_eq!(fixture.evidence.generations(), 1);

    let flow = BuildFlowInput {
        organization_id: fixture.organization_id,
        build_run_id: fixture.build.id,
    };
    let prepared: PrepareStepOutput =
        replay_step(&fixture, &run_id, "prepare", "build_prepare_input", &flow).await?;
    assert!(matches!(prepared, PrepareStepOutput::Ready { .. }));

    let output = attesting
        .output
        .clone()
        .ok_or("attesting build omitted validated output")?;
    let publication: PreparePublicationStepOutput = replay_step(
        &fixture,
        &run_id,
        "publication-target",
        "build_prepare_publication",
        &PreparePublicationStepInput {
            flow: flow.clone(),
            output: output.clone(),
        },
    )
    .await?;
    let PreparePublicationStepOutput::Ready {
        target,
        deadline_at,
    } = publication
    else {
        return Err("durable publication target did not replay as ready".into());
    };
    let artifact = attesting
        .published_artifact
        .clone()
        .ok_or("attesting build omitted published artifact")?;
    let published: PublishStepOutput = replay_step(
        &fixture,
        &run_id,
        "publish",
        "build_publish_output",
        &PublishStepInput {
            flow: flow.clone(),
            output,
            target,
            deadline_at,
        },
    )
    .await?;
    assert!(matches!(
        published,
        PublishStepOutput::Ready {
            artifact: replayed
        } if replayed == artifact
    ));
    let attested: AttestStepOutput = replay_step(
        &fixture,
        &run_id,
        "attest",
        "build_attest_output",
        &AttestStepInput { flow, artifact },
    )
    .await?;
    assert!(matches!(attested, AttestStepOutput::Ready { .. }));
    assert_eq!(fixture.inputs.prepares(), 1);
    assert_eq!(fixture.outputs.validations(), 1);
    assert_eq!(fixture.publisher.publications(), 1);
    assert_eq!(fixture.evidence.generations(), 1);

    drop(engine);
    let engine = FlowEngine::new(store, Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    let pending = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(pending.status, BuildRunStatus::CleanupPending);
    assert!(pending.evidence.is_some());
    assert_eq!(fixture.publisher.publications(), 1);
    assert_eq!(fixture.evidence.generations(), 1);

    let cleanup_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        apply.sequence,
    )
    .await?;
    let cleanup = cleanup_lease
        .commands
        .first()
        .ok_or("missing build Runtime removal")?;
    let NodeCommandPayload::RuntimeRemove { request } = &cleanup.payload else {
        return Err("build cleanup command is not Runtime remove".into());
    };
    acknowledge_removal(&fixture, cleanup, request).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;

    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    let completed = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(completed.status, BuildRunStatus::Succeeded);
    assert!(completed.evidence.is_some());
    Ok(())
}

#[tokio::test]
async fn transient_attestation_failure_retries_without_republishing_or_early_cleanup(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    let apply_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        0,
    )
    .await?;
    let apply = apply_lease.commands.first().ok_or("missing build apply")?;
    let NodeCommandPayload::RuntimeApply { request } = &apply.payload else {
        return Err("build command is not Runtime apply".into());
    };
    record_observation(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        &fixture.capabilities,
        apply,
        succeeded_observation(&request.spec, fixture.outputs.artifact())?,
    )
    .await?;
    fixture
        .evidence
        .fail_once_with(BuildEvidenceGenerationError::Unavailable(
            "test signing provider is temporarily unavailable".into(),
        ));

    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let retrying = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(retrying.status, BuildRunStatus::Attesting);
    assert!(retrying.published_artifact.is_some());
    assert!(retrying.evidence.is_none());
    assert!(retrying.cleanup_command_id.is_none());
    assert!(retrying.failure.is_none());
    assert_eq!(fixture.publisher.publications(), 1);
    assert_eq!(fixture.evidence.generations(), 1);
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Suspended
    );

    engine
        .resume_due_retries(Utc::now() + Duration::seconds(2))
        .await?;
    let pending = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(pending.status, BuildRunStatus::CleanupPending);
    assert!(pending.published_artifact.is_some());
    assert!(pending.evidence.is_some());
    assert!(pending.cleanup_command_id.is_some());
    assert!(pending.failure.is_none());
    assert_eq!(fixture.publisher.publications(), 1);
    assert_eq!(fixture.evidence.generations(), 2);

    let cleanup_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        apply.sequence,
    )
    .await?;
    let cleanup = cleanup_lease
        .commands
        .first()
        .ok_or("missing build Runtime removal")?;
    let NodeCommandPayload::RuntimeRemove { request } = &cleanup.payload else {
        return Err("build cleanup command is not Runtime remove".into());
    };
    acknowledge_removal(&fixture, cleanup, request).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(3))
        .await?;

    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    let completed = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(completed.status, BuildRunStatus::Succeeded);
    assert!(completed.evidence.is_some());
    assert!(completed.failure.is_none());
    Ok(())
}

#[tokio::test]
async fn terminal_attestation_failure_after_cancel_fails_flow_without_releasing_artifact(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    let apply_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        0,
    )
    .await?;
    let apply = apply_lease.commands.first().ok_or("missing build apply")?;
    let NodeCommandPayload::RuntimeApply { request } = &apply.payload else {
        return Err("build command is not Runtime apply".into());
    };
    record_observation(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        &fixture.capabilities,
        apply,
        succeeded_observation(&request.spec, fixture.outputs.artifact())?,
    )
    .await?;

    fixture.publisher.pause_next_publication();
    let resuming_engine = engine.clone();
    let resume = tokio::spawn(async move {
        resuming_engine
            .resume_due_waits(Utc::now() + Duration::seconds(1))
            .await
    });
    fixture.publisher.wait_for_publication().await;
    let mut cancelling = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    let expected = cancelling.aggregate_version;
    cancelling.request_cancellation(Utc::now().max(cancelling.updated_at))?;
    fixture.builds.save(cancelling, expected).await?;
    fixture
        .evidence
        .fail_with(BuildEvidenceGenerationError::Integrity(
            "test signer returned an unverifiable signature".into(),
        ));
    fixture.publisher.resume_publication();
    resume.await??;

    engine
        .resume_due_retries(Utc::now() + Duration::seconds(2))
        .await?;
    let pending = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(pending.status, BuildRunStatus::CleanupPending);
    assert!(pending.published_artifact.is_some());
    assert!(pending.evidence.is_none());
    assert!(pending
        .failure
        .as_deref()
        .is_some_and(|reason| reason.contains("unverifiable signature")));
    assert_eq!(fixture.evidence.generations(), 1);
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Suspended
    );

    let cleanup_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        apply.sequence,
    )
    .await?;
    let cleanup = cleanup_lease
        .commands
        .first()
        .ok_or("missing build Runtime removal")?;
    let NodeCommandPayload::RuntimeRemove { request } = &cleanup.payload else {
        return Err("build cleanup command is not Runtime remove".into());
    };
    acknowledge_removal(&fixture, cleanup, request).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(3))
        .await?;

    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Failed
    );
    let cancelled = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(cancelled.status, BuildRunStatus::Cancelled);
    assert!(cancelled.published_artifact.is_some());
    assert!(cancelled.evidence.is_none());
    assert!(cancelled.failure.is_some());
    Ok(())
}

async fn replay_step<I, O>(
    fixture: &BuildFixture,
    run_id: &str,
    step_id: &str,
    step_name: &str,
    input: &I,
) -> Result<O, Box<dyn std::error::Error>>
where
    I: Serialize,
    O: DeserializeOwned,
{
    let output = fixture
        .runtime
        .run_step(StepInvocation {
            run_id: run_id.into(),
            step_id: step_id.into(),
            step_name: step_name.into(),
            input: serde_json::to_value(input)?,
            history: Vec::new(),
        })
        .await?;
    Ok(serde_json::from_value(output)?)
}
