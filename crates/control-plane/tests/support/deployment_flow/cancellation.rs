use super::*;

pub async fn exercise_pre_dispatch_cancellation(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_uuid: Uuid,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let runtime = DeploymentFlowRuntime::new(
        workload_repository.clone(),
        test_artifact_resolver(),
        node_repository.clone(),
        node_repository,
        Arc::new(a3s_cloud_control_plane::modules::workloads::UnroutedDeploymentRouteUpdater),
        ChronoDuration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 5, 20_000, 5_000, 5, 20_000)?,
    )?;
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(runtime)).await?;
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let operation_id = OperationId::from_uuid(field_uuid(response, "operationId")?);
    let deployment_id = DeploymentId::from_uuid(field_uuid(response, "deploymentId")?);
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    let coordinator = FlowOperationCoordinator::new(
        reconciler,
        &flow,
        Duration::from_millis(5),
        Duration::from_secs(1),
    )?;

    for _ in 0..5 {
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::Cancelled {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let cancelled = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert!(cancelled.node_id.is_none());
    assert!(cancelled.command_id.is_none());
    assert!(cancelled.cleanup_command_id.is_none());
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("cancelled deployment operation has no projection")?
            .status,
        OperationStatus::Cancelled
    );
    Ok(())
}

pub async fn exercise_dispatched_cancellation(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_uuid: Uuid,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if !docker_tests_enabled() {
        return Ok(());
    }

    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    Database::new(PostgresDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "update nodes set state = 'draining', aggregate_version = aggregate_version + 1 where organization_id = ",
            )
            .bind(organization_uuid)
            .append(" and state = 'ready'"),
        )
        .await?;
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&node_repository, organization_id).await?;
    let runtime = DeploymentFlowRuntime::new(
        workload_repository.clone(),
        test_artifact_resolver(),
        node_repository.clone(),
        node_repository.clone(),
        Arc::new(a3s_cloud_control_plane::modules::workloads::UnroutedDeploymentRouteUpdater),
        ChronoDuration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 5, 20_000, 5_000, 5, 20_000)?,
    )?;
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(runtime)).await?;
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let operation_id = OperationId::from_uuid(field_uuid(response, "operationId")?);
    let deployment_id = DeploymentId::from_uuid(field_uuid(response, "deploymentId")?);
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    let coordinator = FlowOperationCoordinator::new(
        reconciler,
        &flow,
        Duration::from_millis(5),
        Duration::from_secs(1),
    )?;

    for _ in 0..8 {
        coordinator.run_once().await?;
        if workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?
            .status
            == DeploymentStatus::Applying
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let applying = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(applying.status, DeploymentStatus::Applying);
    let apply_command_id = applying
        .command_id
        .ok_or("deployment has no apply command")?;
    let apply_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: 0,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            Utc::now(),
            Utc::now() + ChronoDuration::seconds(10),
        )
        .await?;
    let apply_command = apply_lease
        .commands
        .into_iter()
        .find(|command| command.command_id == apply_command_id.as_uuid())
        .ok_or("deployment apply command was not leased")?;
    let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request } = &apply_command.payload
    else {
        return Err("deployment command is not Runtime apply".into());
    };
    let expected_spec = request.spec.clone();

    let state_directory = tempfile::tempdir()?;
    let driver = Arc::new(DockerRuntimeDriver::connect(&DockerConfig {
        socket: docker_socket(),
        namespace: format!(
            "cloud-cancel-{}",
            &Uuid::now_v7().simple().to_string()[..12]
        ),
        operation_timeout_ms: 30_000,
        secret_memory_dir: docker_secret_memory_dir(),
    })?);
    driver.bind_node(node_id.as_uuid()).await?;
    let state: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(
        state_directory.path().join("runtime"),
    ));
    let runtime_driver: Arc<dyn RuntimeDriver> = driver;
    let runtime_client: Arc<dyn RuntimeClient> =
        Arc::new(ManagedRuntimeClient::new(state, runtime_driver));
    let command_executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(state_directory.path().join("journal"), node_id.as_uuid())?,
        runtime_client.clone(),
    );
    let apply_acknowledgement = command_executor.execute(apply_command.clone()).await?;
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        apply_acknowledgement,
    )
    .await?;

    let mut cancelling = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    let expected_version = cancelling.aggregate_version;
    let cancellation_at = Utc::now().max(cancelling.updated_at);
    cancelling.request_cancellation(cancellation_at)?;
    let cancellation_event =
        DeploymentCancellationRequested::envelope(&cancelling, Uuid::now_v7())?;
    workload_repository
        .request_deployment_cancellation(RequestDeploymentCancellationBundle {
            deployment: cancelling,
            expected_version,
            idempotency: IdempotencyRequest::new(
                format!("test.deployment.{deployment_id}.cancellation"),
                "cancel-after-runtime-apply",
                deployment_id.to_string().as_bytes(),
            )?,
            event: cancellation_event,
        })
        .await?;

    let mut cleanup_command_id = None;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(6)).await;
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::CleanupPending {
            cleanup_command_id = deployment.cleanup_command_id;
            break;
        }
    }
    let cleanup_command_id = cleanup_command_id.ok_or("cleanup command was not persisted")?;
    let cleanup_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: apply_command.sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            Utc::now(),
            Utc::now() + ChronoDuration::seconds(10),
        )
        .await?;
    let cleanup_command = cleanup_lease
        .commands
        .into_iter()
        .find(|command| command.command_id == cleanup_command_id.as_uuid())
        .ok_or("Runtime stop command was not leased")?;
    assert!(matches!(
        &cleanup_command.payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { .. }
    ));
    let cleanup_acknowledgement = command_executor.execute(cleanup_command).await?;
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities,
        cleanup_acknowledgement,
    )
    .await?;

    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(6)).await;
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::Cancelled {
            break;
        }
    }

    let cancelled = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert_eq!(cancelled.command_id, Some(apply_command_id));
    assert_eq!(cancelled.cleanup_command_id, Some(cleanup_command_id));
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("cancelled deployment operation has no projection")?
            .status,
        OperationStatus::Cancelled
    );
    match runtime_client.inspect(&expected_spec.unit_id).await? {
        RuntimeInspection::Found { observation, .. } => {
            assert_eq!(observation.state, RuntimeUnitState::Stopped)
        }
        RuntimeInspection::NotFound { .. } => {}
    }
    assert_eq!(
        Database::new(PostgresDialect, executor.clone())
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_commands where correlation_id = ")
                    .bind(operation_id.as_uuid())
                    .append(" and acknowledgement is not null"),
            )
            .await?,
        2
    );
    runtime_client
        .remove(&RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("integration-cleanup-{}", Uuid::now_v7()),
            unit_id: expected_spec.unit_id,
            generation: expected_spec.generation,
            deadline_at_ms: None,
        })
        .await?;
    Ok(())
}
