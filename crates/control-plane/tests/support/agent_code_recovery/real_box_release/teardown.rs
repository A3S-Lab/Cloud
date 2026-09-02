use super::*;

pub(super) async fn request_workload_stop(
    workloads: &PostgresWorkloadRepository,
    organization_id: OrganizationId,
    workload_id: a3s_cloud_control_plane::modules::shared_kernel::domain::WorkloadId,
    requested_at: DateTime<Utc>,
) -> TestResult<OperationId> {
    let mut workload = workloads
        .find_workload(organization_id, workload_id)
        .await?;
    let requested_at = requested_at.max(workload.updated_at + Duration::milliseconds(1));
    let expected_version = workload.aggregate_version;
    workload.request_stop(requested_at)?;
    let operation_id = OperationId::new();
    let operation = OperationRequest::new(
        operation_id,
        organization_id,
        OperationSubject::new("workload", workload_id.as_uuid())?,
        WorkflowIdentity::new(STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION)?,
        json!({
            "operationId": operation_id,
            "organizationId": organization_id,
            "requestedAt": requested_at,
            "workloadId": workload_id,
        }),
        requested_at,
    );
    let event = WorkloadStopRequested::envelope(&workload, &operation, Uuid::now_v7())?;
    workloads
        .request_workload_stop(RequestWorkloadStopBundle {
            workload,
            expected_version,
            operation,
            idempotency: idempotency(
                &format!("test.a0-4.workloads/{workload_id}/stop"),
                "stop-real-box-agent",
                b"stop-real-box-agent",
            )?,
            event,
        })
        .await?;
    Ok(operation_id)
}

pub(super) fn verify_stopped_acknowledgement(
    acknowledgement: &NodeCommandAck,
    spec: &RuntimeUnitSpec,
) -> TestResult {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::RuntimeStopped {
                inspection: RuntimeInspection::Found { observation, .. },
            } if observation.unit_id == spec.unit_id
                && observation.generation == spec.generation
                && observation.state == RuntimeUnitState::Stopped =>
            {
                Ok(())
            }
            result => Err(invalid(format!("real Agent stop returned {result:?}")).into()),
        },
        outcome => Err(invalid(format!("real Agent stop failed: {outcome:?}")).into()),
    }
}

pub(super) async fn drive_until_stopped(
    coordinator: &FlowOperationCoordinator,
    workloads: &PostgresWorkloadRepository,
    operations: &dyn IOperationRepository,
    organization_id: OrganizationId,
    workload_id: a3s_cloud_control_plane::modules::shared_kernel::domain::WorkloadId,
    operation_id: OperationId,
) -> TestResult {
    let deadline = Instant::now() + StdDuration::from_secs(60);
    loop {
        coordinator.run_once().await?;
        let workload = workloads
            .find_workload(organization_id, workload_id)
            .await?;
        let operation = operations.find_projection(operation_id).await?;
        if workload.desired_state == WorkloadDesiredState::Stopped
            && operation.is_some_and(|projection| projection.status == OperationStatus::Succeeded)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(invalid("published Agent stop Flow did not complete").into());
        }
        tokio::time::sleep(StdDuration::from_millis(10)).await;
    }
}

pub(super) async fn enqueue_remove(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    aggregate_id: Uuid,
    correlation_id: Uuid,
    spec: &RuntimeUnitSpec,
) -> TestResult<a3s_cloud_control_plane::modules::fleet::domain::entities::NodeCommand> {
    let issued_at = canonical_timestamp(Utc::now());
    Ok(nodes
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id,
            aggregate_id,
            payload: NodeCommandPayload::RuntimeRemove {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: format!("a0-4:{}:remove", spec.unit_id),
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                    deadline_at_ms: Some(u64::try_from(
                        (issued_at + Duration::minutes(2)).timestamp_millis(),
                    )?),
                },
            },
            issued_at,
            not_after: issued_at + Duration::minutes(2),
            correlation_id,
        })
        .await?
        .value)
}

pub(super) async fn lease_only_command(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
) -> TestResult<NodeCommandEnvelope> {
    let now = canonical_timestamp(Utc::now());
    let lease = nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence,
                max_commands: 1,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(REAL_BOX_COMMAND_LEASE_SECONDS),
        )
        .await?;
    if lease.commands.len() != 1 {
        return Err(invalid("Fleet did not lease the sole Agent Runtime removal").into());
    }
    lease
        .commands
        .into_iter()
        .next()
        .ok_or_else(|| invalid("Fleet omitted the Agent Runtime removal").into())
}

pub(super) fn verify_removed_acknowledgement(
    acknowledgement: &NodeCommandAck,
    spec: &RuntimeUnitSpec,
) -> TestResult {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::RuntimeRemoved { removal }
                if removal.unit_id == spec.unit_id && removal.generation == spec.generation =>
            {
                Ok(())
            }
            result => Err(invalid(format!("real Agent remove returned {result:?}")).into()),
        },
        outcome => Err(invalid(format!("real Agent remove failed: {outcome:?}")).into()),
    }
}

pub(super) fn verify_clean_state(home: &Path, node_state: &Path) -> TestResult {
    let state_path = home.join("boxes.json");
    let store = BoxStateStore::load_readonly(&state_path)?;
    if !store.records().is_empty()
        || directory_has_entries(&home.join("boxes"))?
        || directory_has_entries(&home.join("runtime-secrets"))?
        || directory_has_entries(&node_state.join("artifacts/mounts"))?
        || directory_has_entries(&node_state.join("artifacts/outputs"))?
        || directory_has_entries(&node_state.join("artifacts/blobs/sha256"))?
        || directory_has_entries(&node_state.join("artifacts/staging"))?
    {
        return Err(
            invalid("published Agent cleanup retained Box, Secret, or Artifact state").into(),
        );
    }
    for path in [
        state_path,
        home.join("boxes.json.lock"),
        home.join("boxes.json.tmp"),
    ] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn directory_has_entries(path: &Path) -> io::Result<bool> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().transpose()?.is_some()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn require_gate() -> TestResult {
    if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
        return Err(invalid("dedicated A0.4 gate did not set A3S_CLOUD_TEST_BOX=1").into());
    }
    Ok(())
}

pub(super) fn dedicated_box_home() -> TestResult<PathBuf> {
    let configured = PathBuf::from(
        std::env::var_os("A3S_HOME")
            .ok_or_else(|| invalid("dedicated A0.4 gate did not configure A3S_HOME"))?,
    );
    if !configured.is_absolute() || configured.canonicalize()? != configured {
        return Err(invalid("dedicated A0.4 A3S_HOME must be absolute and canonical").into());
    }
    Ok(configured)
}
