use super::support::*;
use crate::modules::artifacts::domain::{
    BuildOutputValidationError, BuildRunStatus, IBuildRunRepository,
};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{
    NodeBoxBuildCancelResult, NodeBoxBuildCancellation, NodeBoxBuildInspection,
    NodeBoxBuildOperationCancellation, NodeBoxBuildOperationRemoval, NodeBoxBuildPhase,
    NodeBoxBuildRemoveResult, NodeBoxBuildRequest, NodeBoxBuildStartResult, NodeCommandAck,
    NodeCommandEnvelope, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_flow::{FlowEngine, FlowError, WorkflowRunStatus};
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn dispatch_replay_reuses_the_exact_box_start_command(
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
    let command_id = NodeCommandId::from_uuid(Uuid::new_v5(
        &fixture.build.id.as_uuid(),
        b"box-build-start",
    ));
    let before_restart = fixture
        .nodes
        .find_command(fixture.node_id, command_id)
        .await?
        .ok_or("Box start command was not persisted before the crash")?;

    drop(engine);
    let engine = FlowEngine::new(store, Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id, workflow_spec(), fixture.input())
        .await?;
    let after_restart = fixture
        .nodes
        .find_command(fixture.node_id, command_id)
        .await?
        .ok_or("Box start command disappeared after replay")?;
    assert_eq!(after_restart, before_restart);
    assert_eq!(fixture.inputs.prepares(), 1);

    let leased = lease_one(&fixture, 0).await?;
    assert_eq!(leased.command_id, command_id.as_uuid());
    assert!(matches!(
        leased.payload,
        NodeCommandPayload::BoxBuildStart { .. }
    ));
    Ok(())
}

#[tokio::test]
async fn process_restart_reuses_the_exact_box_cleanup_command(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let store = Arc::new(FailOnceStepCompletionStore::new(
        "cleanup-cancel-dispatch-1",
    ));
    let engine = FlowEngine::new(store.clone(), Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    let start = lease_one(&fixture, 0).await?;
    let build_request = request(&start)?.clone();
    acknowledge(
        &fixture,
        &start,
        NodeCommandResult::BoxBuildStarted {
            started: NodeBoxBuildStartResult {
                binding_digest: build_request.binding_digest()?,
                phase: NodeBoxBuildPhase::Running,
            },
        },
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let inspect = lease_one(&fixture, start.sequence).await?;
    let output = box_output_for(&build_request, fixture.outputs.artifact())?;
    acknowledge(
        &fixture,
        &inspect,
        NodeCommandResult::BoxBuildInspected {
            inspection: Box::new(NodeBoxBuildInspection::Succeeded {
                binding_digest: build_request.binding_digest()?,
                output: Box::new(output.clone()),
            }),
        },
    )
    .await?;

    let failure = engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await
        .expect_err("injected crash must interrupt cleanup dispatch persistence");
    assert!(matches!(failure, FlowError::Store(_)));
    let cleanup_command_id = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?
        .cleanup_command_id
        .ok_or("Box cleanup command was not persisted before the crash")?;
    let before_restart = fixture
        .nodes
        .find_command(fixture.node_id, cleanup_command_id)
        .await?
        .ok_or("Box cleanup command was not queued before the crash")?;

    drop(engine);
    let engine = FlowEngine::new(store, Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    let after_restart = fixture
        .nodes
        .find_command(fixture.node_id, cleanup_command_id)
        .await?
        .ok_or("Box cleanup command disappeared after restart")?;
    assert_eq!(after_restart, before_restart);

    let cancel = lease_one(&fixture, inspect.sequence).await?;
    assert_eq!(cancel.command_id, cleanup_command_id.as_uuid());
    finish_cleanup(
        &fixture,
        &engine,
        &cancel,
        NodeBoxBuildInspection::Succeeded {
            binding_digest: build_request.binding_digest()?,
            output: Box::new(output),
        },
    )
    .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(fixture.publisher.publications(), 1);
    assert_eq!(fixture.evidence.generations(), 1);
    assert_eq!(fixture.inputs.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn box_build_completes_only_after_cancel_inspect_remove_cleanup(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime.clone()));
    let (output, cancel) = drive_to_cleanup(&fixture, &engine, &run_id).await?;

    assert!(matches!(
        cancel.payload,
        NodeCommandPayload::BoxBuildCancel { .. }
    ));
    finish_cleanup(
        &fixture,
        &engine,
        &cancel,
        NodeBoxBuildInspection::Succeeded {
            binding_digest: request(&cancel)?.binding_digest()?,
            output: Box::new(output),
        },
    )
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
    assert!(completed.published_artifact.is_some());
    assert!(completed.evidence.is_some());
    assert!(completed.finished_at.is_some());
    assert_eq!(fixture.outputs.validations(), 1);
    assert_eq!(fixture.publisher.publications(), 1);
    assert_eq!(fixture.evidence.generations(), 1);
    assert_eq!(fixture.inputs.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn offline_cleanup_polls_the_same_command_until_its_ttl_expires(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime.clone()));
    let (_, cancel) = drive_to_cleanup(&fixture, &engine, &run_id).await?;
    let expected = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?
        .cleanup_command_id;

    for seconds in 3..7 {
        engine
            .resume_due_waits(Utc::now() + Duration::seconds(seconds))
            .await?;
    }

    let pending = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(pending.cleanup_command_id, expected);
    assert_eq!(pending.status, BuildRunStatus::CleanupPending);
    assert!(lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        cancel.sequence,
    )
    .await?
    .commands
    .is_empty());
    Ok(())
}

#[tokio::test]
async fn cancellation_uses_the_same_box_cleanup_state_machine(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(None).await?;
    let run_id = fixture.build.operation_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime.clone()));
    engine
        .start_with_id(run_id.clone(), workflow_spec(), fixture.input())
        .await?;
    let start = lease_one(&fixture, 0).await?;

    let mut build = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    let expected = build.aggregate_version;
    build.request_cancellation(Utc::now().max(build.updated_at))?;
    fixture.builds.save(build, expected).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let cancel = lease_one(&fixture, start.sequence).await?;
    assert!(matches!(
        cancel.payload,
        NodeCommandPayload::BoxBuildCancel { .. }
    ));

    finish_cleanup(
        &fixture,
        &engine,
        &cancel,
        NodeBoxBuildInspection::Cancelled {
            binding_digest: request(&cancel)?.binding_digest()?,
            message: "cancelled by Cloud".into(),
        },
    )
    .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    let cancelled = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert_eq!(cancelled.status, BuildRunStatus::Cancelled);
    assert!(cancelled.failure.is_none());
    Ok(())
}

#[tokio::test]
async fn rejected_box_output_fails_only_after_the_same_cleanup_chain(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BuildFixture::create(Some(BuildOutputValidationError::Integrity(
        "tampered OCI graph".into(),
    )))
    .await?;
    let run_id = fixture.build.operation_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime.clone()));
    let (output, cancel) = drive_to_cleanup(&fixture, &engine, &run_id).await?;
    let pending = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    assert!(pending
        .failure
        .as_deref()
        .is_some_and(|reason| reason.contains("tampered OCI graph")));
    assert_eq!(fixture.publisher.publications(), 0);

    finish_cleanup(
        &fixture,
        &engine,
        &cancel,
        NodeBoxBuildInspection::Succeeded {
            binding_digest: request(&cancel)?.binding_digest()?,
            output: Box::new(output),
        },
    )
    .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Failed
    );
    assert_eq!(
        fixture
            .builds
            .find(fixture.organization_id, fixture.build.id)
            .await?
            .status,
        BuildRunStatus::Failed
    );
    Ok(())
}

async fn drive_to_cleanup(
    fixture: &BuildFixture,
    engine: &FlowEngine,
    run_id: &str,
) -> Result<
    (a3s_cloud_contracts::NodeBoxBuildOutput, NodeCommandEnvelope),
    Box<dyn std::error::Error>,
> {
    engine
        .start_with_id(run_id.to_owned(), workflow_spec(), fixture.input())
        .await?;
    let start = lease_one(fixture, 0).await?;
    let build_request = request(&start)?.clone();
    acknowledge(
        fixture,
        &start,
        NodeCommandResult::BoxBuildStarted {
            started: NodeBoxBuildStartResult {
                binding_digest: build_request.binding_digest()?,
                phase: NodeBoxBuildPhase::Running,
            },
        },
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;

    let inspect = lease_one(fixture, start.sequence).await?;
    assert!(matches!(
        inspect.payload,
        NodeCommandPayload::BoxBuildInspect { .. }
    ));
    let output = box_output_for(&build_request, fixture.outputs.artifact())?;
    acknowledge(
        fixture,
        &inspect,
        NodeCommandResult::BoxBuildInspected {
            inspection: Box::new(NodeBoxBuildInspection::Succeeded {
                binding_digest: build_request.binding_digest()?,
                output: Box::new(output.clone()),
            }),
        },
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    let cancel = lease_one(fixture, inspect.sequence).await?;
    Ok((output, cancel))
}

async fn finish_cleanup(
    fixture: &BuildFixture,
    engine: &FlowEngine,
    cancel: &NodeCommandEnvelope,
    terminal_inspection: NodeBoxBuildInspection,
) -> Result<(), Box<dyn std::error::Error>> {
    let build_request = request(cancel)?.clone();
    acknowledge(
        fixture,
        cancel,
        NodeCommandResult::BoxBuildCancelled {
            cancelled: NodeBoxBuildCancelResult {
                binding_digest: build_request.binding_digest()?,
                operations: build_request
                    .plans
                    .iter()
                    .map(|plan| NodeBoxBuildOperationCancellation {
                        operation_id: plan.operation_id.clone(),
                        outcome: NodeBoxBuildCancellation::Requested,
                    })
                    .collect(),
            },
        },
    )
    .await?;
    resume_cleanup_advance(engine, 3).await?;
    let inspect = lease_one(fixture, cancel.sequence).await?;
    assert!(matches!(
        inspect.payload,
        NodeCommandPayload::BoxBuildInspect { .. }
    ));
    acknowledge(
        fixture,
        &inspect,
        NodeCommandResult::BoxBuildInspected {
            inspection: Box::new(terminal_inspection),
        },
    )
    .await?;
    resume_cleanup_advance(engine, 5).await?;
    let remove = lease_one(fixture, inspect.sequence).await?;
    assert!(matches!(
        remove.payload,
        NodeCommandPayload::BoxBuildRemove { .. }
    ));
    acknowledge(
        fixture,
        &remove,
        NodeCommandResult::BoxBuildRemoved {
            removed: NodeBoxBuildRemoveResult {
                binding_digest: build_request.binding_digest()?,
                operations: build_request
                    .plans
                    .iter()
                    .map(|plan| NodeBoxBuildOperationRemoval {
                        operation_id: plan.operation_id.clone(),
                        removed: true,
                    })
                    .collect(),
                assembly_removed: build_request.assembly_reference.is_some(),
            },
        },
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(7))
        .await?;
    Ok(())
}

async fn resume_cleanup_advance(
    engine: &FlowEngine,
    seconds: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(seconds))
        .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(seconds + 1))
        .await?;
    Ok(())
}

async fn lease_one(
    fixture: &BuildFixture,
    after_sequence: u64,
) -> Result<NodeCommandEnvelope, Box<dyn std::error::Error>> {
    let response = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        after_sequence,
    )
    .await?;
    if response.commands.len() != 1 {
        return Err(format!(
            "expected one Box command after sequence {after_sequence}, got {}",
            response.commands.len()
        )
        .into());
    }
    Ok(response.commands.into_iter().next().expect("one command"))
}

fn request(
    command: &NodeCommandEnvelope,
) -> Result<&NodeBoxBuildRequest, Box<dyn std::error::Error>> {
    match &command.payload {
        NodeCommandPayload::BoxBuildStart { request }
        | NodeCommandPayload::BoxBuildInspect { request }
        | NodeCommandPayload::BoxBuildCancel { request }
        | NodeCommandPayload::BoxBuildRemove { request } => Ok(request),
        _ => Err("command is not a Box build command".into()),
    }
}

async fn acknowledge(
    fixture: &BuildFixture,
    command: &NodeCommandEnvelope,
    result: NodeCommandResult,
) -> Result<(), Box<dyn std::error::Error>> {
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
                completed_at: Utc::now(),
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(result),
                },
            },
            Utc::now(),
        )
        .await?;
    Ok(())
}
