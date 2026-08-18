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
        DeploymentFlowDependencies::new(
            workload_repository.clone(),
            Arc::new(PostgresResourceClaimRepository::new(executor.clone())),
            test_artifact_resolver(),
            node_repository.clone(),
            node_repository,
            Arc::new(a3s_cloud_control_plane::modules::workloads::UnroutedDeploymentRouteUpdater),
        ),
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
