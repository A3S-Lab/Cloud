use super::support::*;
use crate::modules::artifacts::domain::{
    BuildOutputValidationError, BuildRunStatus, IBuildRunRepository,
};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, OrganizationId, ProjectId, SourceRevisionId,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_flow::{FlowEngine, FlowError, FlowEventStore, WorkflowRunStatus};
use a3s_runtime::contract::{NetworkMode, RuntimeRemoval};
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn build_flow_replays_dispatch_and_completes_only_after_exact_runtime_removal(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let store = Arc::new(FailOnceStepCompletionStore::new("dispatch"));
    let engine = FlowEngine::new(store.clone(), Arc::new(fixture.runtime.clone()));

    let failure = engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await
        .expect_err("injected crash must interrupt dispatch completion persistence");
    assert!(matches!(failure, FlowError::Store(_)));
    let expected_apply_id = NodeCommandId::from_uuid(fixture.build.id.as_uuid());
    let command_before_restart = fixture
        .nodes
        .find_command(fixture.node_id, expected_apply_id)
        .await?
        .ok_or("build apply side effect was not persisted before the injected crash")?;
    assert_eq!(fixture.inputs.prepares(), 1);

    drop(engine);
    let engine = FlowEngine::new(store.clone(), Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    assert_eq!(
        fixture
            .nodes
            .find_command(fixture.node_id, expected_apply_id)
            .await?
            .ok_or("build apply command disappeared after restart")?,
        command_before_restart
    );
    assert_eq!(fixture.inputs.prepares(), 1);
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Suspended
    );

    let apply_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        0,
    )
    .await?;
    assert_eq!(apply_lease.commands.len(), 1);
    let apply = apply_lease.commands.first().ok_or("missing build apply")?;
    assert_eq!(apply.command_id, expected_apply_id.as_uuid());
    let NodeCommandPayload::RuntimeApply { request } = &apply.payload else {
        return Err("build command is not Runtime apply".into());
    };
    assert_eq!(
        request.request_id,
        format!("build:{}:apply", fixture.build.id)
    );
    assert_eq!(request.spec.network.mode, NetworkMode::None);
    assert_eq!(request.spec.artifact.media_type, BUILDER_MEDIA_TYPE);
    assert_eq!(request.spec.mounts.len(), 2);
    assert_eq!(request.spec.outputs.len(), 1);
    assert!(request
        .spec
        .process
        .args
        .windows(2)
        .any(|arguments| arguments == ["--opt", "force-network-mode=none"]));
    assert_eq!(
        fixture
            .builds
            .find(fixture.organization_id, fixture.build.id)
            .await?
            .node_id,
        Some(fixture.node_id)
    );

    record_observation(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        &fixture.capabilities,
        apply,
        succeeded_observation(&request.spec, fixture.outputs.artifact())?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;

    let cleanup_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        apply.sequence,
    )
    .await?;
    assert_eq!(cleanup_lease.commands.len(), 1);
    let cleanup = cleanup_lease
        .commands
        .first()
        .ok_or("missing build Runtime removal")?;
    let expected_cleanup_id = NodeCommandId::from_uuid(Uuid::new_v5(
        &fixture.build.id.as_uuid(),
        b"runtime-remove:1",
    ));
    assert_eq!(cleanup.command_id, expected_cleanup_id.as_uuid());
    let NodeCommandPayload::RuntimeRemove {
        request: removal_request,
    } = &cleanup.payload
    else {
        return Err("build cleanup command is not Runtime remove".into());
    };
    assert_eq!(
        removal_request.request_id,
        format!("build:{}:remove:1", fixture.build.id)
    );
    assert_eq!(removal_request.unit_id, request.spec.unit_id);
    assert_eq!(removal_request.generation, request.spec.generation);
    acknowledge_removal(&fixture, cleanup, removal_request).await?;

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
    assert_eq!(completed.cleanup_command_id, Some(expected_cleanup_id));
    assert!(completed.finished_at.is_some());
    assert_eq!(fixture.outputs.validations(), 1);
    assert_eq!(fixture.inputs.removals(), 1);

    let history_length = store.list(&run_id).await?.len();
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    assert_eq!(store.list(&run_id).await?.len(), history_length);
    assert_eq!(fixture.inputs.prepares(), 1);
    assert_eq!(fixture.outputs.validations(), 1);
    assert_eq!(fixture.inputs.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn rejected_runtime_output_is_failed_only_after_cleanup(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(Some(BuildOutputValidationError::Integrity(
        "tampered OCI graph".into(),
    )))
    .await?;
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
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;

    let pending = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(pending.status, BuildRunStatus::CleanupPending);
    assert!(pending
        .failure
        .as_deref()
        .is_some_and(|reason| reason.contains("tampered OCI graph")));
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
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;

    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Failed
    );
    let failed = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(failed.status, BuildRunStatus::Failed);
    assert!(failed.finished_at.is_some());
    assert_eq!(fixture.outputs.validations(), 1);
    assert_eq!(fixture.inputs.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn flow_rejects_operation_and_source_ownership_changes_before_checkout(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime.clone()));
    let wrong_run_id = Uuid::now_v7().to_string();
    engine
        .start_with_id(wrong_run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    assert_eq!(
        engine.snapshot(&wrong_run_id).await?.status,
        WorkflowRunStatus::Failed
    );
    assert_eq!(fixture.inputs.prepares(), 0);

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let source_revision_id = SourceRevisionId::new();
    let accepted_at = Utc::now() - Duration::seconds(1);
    let mismatched_revision = revision(
        organization_id,
        ProjectId::new(),
        environment_id,
        source_revision_id,
        accepted_at,
    )?;
    accept_revision(&fixture.sources, mismatched_revision).await?;
    fixture
        .builds
        .add_source_revision(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            accepted_at,
        )
        .await;
    let foreign_build = fixture
        .builds
        .reserve_pending(1, accepted_at)
        .await?
        .pop()
        .ok_or("mismatched source build was not reserved")?;
    engine
        .start_with_id(
            foreign_build.operation_id.to_string(),
            workflow_spec(),
            serde_json::json!({
                "organizationId": organization_id,
                "buildRunId": foreign_build.id,
            }),
        )
        .await?;
    assert_eq!(
        engine
            .snapshot(&foreign_build.operation_id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Failed
    );
    assert_eq!(fixture.inputs.prepares(), 0);
    Ok(())
}

async fn acknowledge_removal(
    fixture: &BuildFixture,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    request: &a3s_runtime::contract::RuntimeActionRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let completed_at = Utc::now();
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id: command.lease_id,
                node_id: command.node_id,
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::RuntimeRemoved {
                        removal: RuntimeRemoval {
                            schema: RuntimeRemoval::SCHEMA.into(),
                            request_id: request.request_id.clone(),
                            unit_id: request.unit_id.clone(),
                            generation: request.generation,
                            removed_at_ms: u64::try_from(completed_at.timestamp_millis())?,
                            already_absent: false,
                        },
                    }),
                },
            },
            completed_at,
        )
        .await?;
    Ok(())
}
