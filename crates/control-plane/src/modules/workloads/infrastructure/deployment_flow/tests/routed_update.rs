use super::*;

#[tokio::test]
async fn routed_update_waits_for_exact_gateway_ack_and_retires_the_previous_runtime_once(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let compiler = gateway_compiler()?;
    let route_port: Arc<dyn IEdgeRepository> = routes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
    let gateway_commands: Arc<dyn crate::modules::edge::domain::services::IGatewayCommandQueue> =
        Arc::new(FleetGatewayCommandQueue::new(Arc::clone(&control_port)));
    let route_updates = Arc::new(EdgeDeploymentRouteUpdater::new(
        route_port,
        Arc::clone(&control_port),
        gateway_commands,
        compiler.clone(),
        Duration::seconds(5),
    )?);
    let runtime = DeploymentFlowRuntime::new(
        workloads.clone(),
        Arc::new(UnusedArtifactResolver),
        nodes.clone(),
        control_port,
        route_updates,
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 1, 10_000, 5_000, 1, 10_000)?,
    )?;
    let store = Arc::new(FailOnceStepCompletionStore::new("activate"));
    let engine = FlowEngine::new(store.clone(), Arc::new(runtime.clone()));

    let first = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("routed update fixture")?,
            base,
        ),
        1,
        'a',
        base,
        "routed-update-first",
    )?;
    let first_revision = first.revision.clone();
    let first_deployment_id = first.deployment.id;
    workloads.create_deployment(first).await?;
    let mut first_deployment = workloads
        .find_deployment(organization_id, first_deployment_id)
        .await?;
    first_deployment = workloads
        .mark_resolving(
            first_deployment.id,
            first_deployment.aggregate_version,
            Utc::now().max(first_deployment.updated_at),
        )
        .await?;
    first_deployment = workloads
        .assign_node(
            first_deployment.id,
            first_deployment.aggregate_version,
            node_id,
            Utc::now().max(first_deployment.updated_at),
        )
        .await?;
    first_deployment = workloads
        .mark_dispatched(
            first_deployment.id,
            first_deployment.aggregate_version,
            NodeCommandId::new(),
            Utc::now().max(first_deployment.updated_at),
        )
        .await?;
    first_deployment = workloads
        .mark_verifying(
            first_deployment.id,
            first_deployment.aggregate_version,
            Utc::now().max(first_deployment.updated_at),
        )
        .await?;
    let (active_workload, first_deployment) = workloads
        .activate(
            first_deployment.id,
            first_deployment.aggregate_version,
            false,
            Utc::now().max(first_deployment.updated_at),
        )
        .await?;
    assert_eq!(first_deployment.status, DeploymentStatus::Active);
    let initial_route = publish_active_route(
        &routes,
        &compiler,
        &active_workload,
        first_revision.id,
        node_id,
        Utc::now().max(first_deployment.updated_at),
    )
    .await?;
    assert_eq!(initial_route.state, RouteState::Active);

    let rejected = deployment_bundle(
        active_workload,
        2,
        'b',
        Utc::now().max(initial_route.updated_at),
        "routed-update-rejected",
    )?;
    let rejected_revision = rejected.revision.clone();
    let rejected_deployment = rejected.deployment.clone();
    let rejected_operation = rejected.operation.clone();
    workloads.create_deployment(rejected).await?;
    engine
        .start_with_id(
            rejected_operation.id.to_string(),
            workflow_spec(),
            rejected_operation.input.clone(),
        )
        .await?;
    let rejected_apply = lease(&nodes, node_id, agent_instance_id, 0).await?;
    assert_eq!(rejected_apply.commands.len(), 1);
    let rejected_apply_command = &rejected_apply.commands[0];
    let rejected_spec = project_runtime_spec(&rejected_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        rejected_apply_command,
        healthy_observation(&rejected_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let rejected_cutover = routes
        .find_gateway_route_cutover(organization_id, rejected_deployment.id)
        .await?
        .ok_or("rejected update did not stage a Gateway cutover")?;
    let rejected_gateway = lease(
        &nodes,
        node_id,
        agent_instance_id,
        rejected_apply_command.sequence,
    )
    .await?;
    assert_eq!(rejected_gateway.commands.len(), 1);
    assert_eq!(
        rejected_gateway.commands[0].command_id,
        rejected_cutover.gateway_command_id.as_uuid()
    );
    assert!(matches!(
        rejected_gateway.commands[0].payload,
        a3s_cloud_contracts::NodeCommandPayload::GatewaySnapshotInstall { .. }
    ));
    assert_eq!(
        routes.find_route(organization_id, initial_route.id).await?,
        initial_route
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, rejected_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    let mut wrong = cutover_acknowledgement(&rejected_cutover, GatewayAckState::Applied);
    wrong.snapshot_digest = format!("sha256:{}", "f".repeat(64));
    assert!(routes
        .project_gateway_acknowledgement(&wrong, wrong.acknowledged_at)
        .await
        .is_err());
    assert_eq!(
        routes.find_route(organization_id, initial_route.id).await?,
        initial_route
    );
    let rejected_ack = cutover_acknowledgement(&rejected_cutover, GatewayAckState::Rejected);
    routes
        .project_gateway_acknowledgement(
            &rejected_ack,
            rejected_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    assert_eq!(
        engine
            .snapshot(&rejected_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Failed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, rejected_deployment.id)
            .await?
            .status,
        DeploymentStatus::Failed
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, rejected_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    assert_eq!(
        routes.find_route(organization_id, initial_route.id).await?,
        initial_route
    );

    let selected_workload = workloads
        .find_workload(organization_id, rejected_deployment.workload_id)
        .await?;
    let accepted = deployment_bundle(
        selected_workload,
        3,
        'c',
        Utc::now(),
        "routed-update-accepted",
    )?;
    let accepted_revision = accepted.revision.clone();
    let accepted_deployment = accepted.deployment.clone();
    let accepted_operation = accepted.operation.clone();
    workloads.create_deployment(accepted).await?;
    engine
        .start_with_id(
            accepted_operation.id.to_string(),
            workflow_spec(),
            accepted_operation.input.clone(),
        )
        .await?;
    let accepted_apply = lease(
        &nodes,
        node_id,
        agent_instance_id,
        rejected_gateway.commands[0].sequence,
    )
    .await?;
    assert_eq!(accepted_apply.commands.len(), 1);
    let accepted_apply_command = &accepted_apply.commands[0];
    let accepted_spec = project_runtime_spec(&accepted_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        accepted_apply_command,
        healthy_observation(&accepted_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let accepted_cutover = routes
        .find_gateway_route_cutover(organization_id, accepted_deployment.id)
        .await?
        .ok_or("accepted update did not stage a Gateway cutover")?;
    let accepted_gateway = lease(
        &nodes,
        node_id,
        agent_instance_id,
        accepted_apply_command.sequence,
    )
    .await?;
    assert_eq!(accepted_gateway.commands.len(), 1);
    assert_eq!(
        accepted_gateway.commands[0].command_id,
        accepted_cutover.gateway_command_id.as_uuid()
    );
    assert_eq!(
        routes.find_route(organization_id, initial_route.id).await?,
        initial_route
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, accepted_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    issue_cutover_certificate(&routes, &accepted_cutover).await?;
    let applied_ack = cutover_acknowledgement(&accepted_cutover, GatewayAckState::Applied);
    routes
        .project_gateway_acknowledgement(
            &applied_ack,
            applied_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    let cutover_route = routes.find_route(organization_id, initial_route.id).await?;
    assert_eq!(cutover_route.workload_revision_id, accepted_revision.id);
    assert_eq!(cutover_route.upstream.as_str(), "http://127.0.0.1:49152/");
    assert_eq!(
        workloads
            .find_workload(organization_id, accepted_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );

    let activation_failure = engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await
        .expect_err("activation completion loss must interrupt the Flow");
    assert!(matches!(activation_failure, FlowError::Store(_)));
    let retiring = workloads
        .find_deployment(organization_id, accepted_deployment.id)
        .await?;
    assert_eq!(retiring.status, DeploymentStatus::Retiring);
    assert_eq!(
        workloads
            .find_workload(organization_id, accepted_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(accepted_revision.id)
    );
    assert!(retiring.retirement_command_id.is_none());
    assert!(store
        .list(&accepted_operation.id.to_string())
        .await?
        .iter()
        .any(|event| matches!(
            &event.event,
            FlowEvent::StepStarted { step_id, .. } if step_id == "activate"
        )));
    assert!(!store
        .list(&accepted_operation.id.to_string())
        .await?
        .iter()
        .any(|event| matches!(
            &event.event,
            FlowEvent::StepCompleted { step_id, .. } if step_id == "activate"
        )));
    assert!(lease(
        &nodes,
        node_id,
        agent_instance_id,
        accepted_gateway.commands[0].sequence,
    )
    .await?
    .commands
    .is_empty());

    drop(engine);
    let engine = FlowEngine::new(store, Arc::new(runtime));
    engine
        .start_with_id(
            accepted_operation.id.to_string(),
            workflow_spec(),
            accepted_operation.input.clone(),
        )
        .await?;
    let retirement_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        accepted_gateway.commands[0].sequence,
    )
    .await?;
    assert_eq!(retirement_lease.commands.len(), 1);
    let retirement_command = &retirement_lease.commands[0];
    match &retirement_command.payload {
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { request } => {
            assert_eq!(
                request.unit_id,
                project_runtime_spec(&first_revision)?.unit_id
            );
            assert_eq!(
                request.generation,
                project_runtime_spec(&first_revision)?.generation
            );
        }
        _ => return Err("update retirement dispatched a non-stop command".into()),
    }
    let retiring = workloads
        .find_deployment(organization_id, accepted_deployment.id)
        .await?;
    assert_eq!(
        retiring.retirement_command_id.map(|id| id.as_uuid()),
        Some(retirement_command.command_id)
    );
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        retirement_command,
        stopped_observation(&project_runtime_spec(&first_revision)?)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(3))
        .await?;
    assert_eq!(
        engine
            .snapshot(&accepted_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Completed
    );
    let active = workloads
        .find_deployment(organization_id, accepted_deployment.id)
        .await?;
    assert_eq!(active.status, DeploymentStatus::Active);
    assert_eq!(
        active.retirement_command_id.map(|id| id.as_uuid()),
        Some(retirement_command.command_id)
    );
    let history_length = engine
        .history(&accepted_operation.id.to_string())
        .await?
        .len();
    engine
        .start_with_id(
            accepted_operation.id.to_string(),
            workflow_spec(),
            accepted_operation.input,
        )
        .await?;
    assert_eq!(
        engine
            .history(&accepted_operation.id.to_string())
            .await?
            .len(),
        history_length
    );
    assert!(lease(
        &nodes,
        node_id,
        agent_instance_id,
        retirement_command.sequence,
    )
    .await?
    .commands
    .is_empty());

    let rollback_workload = workloads
        .find_workload(organization_id, accepted_deployment.workload_id)
        .await?;
    let rollback = rollback_deployment_bundle(
        rollback_workload,
        &first_revision,
        4,
        Utc::now(),
        "routed-update-rollback",
    )?;
    let rollback_revision = rollback.revision.clone();
    let rollback_deployment = rollback.deployment.clone();
    let rollback_operation = rollback.operation.clone();
    assert_eq!(rollback_revision.template, first_revision.template);
    assert_eq!(
        rollback_revision.template_digest,
        first_revision.template_digest
    );
    workloads.create_deployment(rollback).await?;
    engine
        .start_with_id(
            rollback_operation.id.to_string(),
            workflow_spec(),
            rollback_operation.input.clone(),
        )
        .await?;
    let rollback_apply = lease(
        &nodes,
        node_id,
        agent_instance_id,
        retirement_command.sequence,
    )
    .await?;
    assert_eq!(rollback_apply.commands.len(), 1);
    let rollback_apply_command = &rollback_apply.commands[0];
    let rollback_spec = project_runtime_spec(&rollback_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        rollback_apply_command,
        healthy_observation(&rollback_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let rollback_cutover = routes
        .find_gateway_route_cutover(organization_id, rollback_deployment.id)
        .await?
        .ok_or("rollback did not stage a Gateway cutover")?;
    let rollback_gateway = lease(
        &nodes,
        node_id,
        agent_instance_id,
        rollback_apply_command.sequence,
    )
    .await?;
    assert_eq!(rollback_gateway.commands.len(), 1);
    assert_eq!(
        rollback_gateway.commands[0].command_id,
        rollback_cutover.gateway_command_id.as_uuid()
    );
    assert_eq!(
        routes.find_route(organization_id, initial_route.id).await?,
        cutover_route
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, rollback_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(accepted_revision.id)
    );

    issue_cutover_certificate(&routes, &rollback_cutover).await?;
    let rollback_ack = cutover_acknowledgement(&rollback_cutover, GatewayAckState::Applied);
    routes
        .project_gateway_acknowledgement(
            &rollback_ack,
            rollback_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    assert_eq!(
        routes
            .find_route(organization_id, initial_route.id)
            .await?
            .workload_revision_id,
        rollback_revision.id
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, rollback_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(rollback_revision.id)
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, rollback_deployment.id)
            .await?
            .status,
        DeploymentStatus::Retiring
    );

    let rollback_retirement = lease(
        &nodes,
        node_id,
        agent_instance_id,
        rollback_gateway.commands[0].sequence,
    )
    .await?;
    assert_eq!(rollback_retirement.commands.len(), 1);
    let rollback_retirement_command = &rollback_retirement.commands[0];
    match &rollback_retirement_command.payload {
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { request } => {
            assert_eq!(
                request.unit_id,
                project_runtime_spec(&accepted_revision)?.unit_id
            );
            assert_eq!(
                request.generation,
                project_runtime_spec(&accepted_revision)?.generation
            );
        }
        _ => return Err("rollback retirement dispatched a non-stop command".into()),
    }
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        rollback_retirement_command,
        stopped_observation(&project_runtime_spec(&accepted_revision)?)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(3))
        .await?;
    assert_eq!(
        engine
            .snapshot(&rollback_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, rollback_deployment.id)
            .await?
            .status,
        DeploymentStatus::Active
    );
    assert!(lease(
        &nodes,
        node_id,
        agent_instance_id,
        rollback_retirement_command.sequence,
    )
    .await?
    .commands
    .is_empty());
    Ok(())
}
