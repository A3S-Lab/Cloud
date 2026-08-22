use super::*;
use crate::modules::workloads::domain::events::{
    WorkloadDeploymentAvailabilityImpact, WorkloadDeploymentFailurePhase,
    WorkloadDeploymentHealthChanged, WorkloadDeploymentHealthStatus,
};

#[tokio::test]
async fn workload_repository_emits_bounded_rollout_health_facts_once(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let workload = Workload::create(
        WorkloadId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("rollout health facts")?,
        now,
    );
    let repository = InMemoryWorkloadRepository::new();

    let first = deployment_bundle(workload.clone(), 1, '1', now, "rollout-health-1")?;
    let first_operation_id = first.operation.id;
    let first_deployment_id = first.deployment.id;
    let first_revision_id = first.revision.id;
    repository.create_deployment(first).await?;
    let private_failure = "private registry response must not escape";
    let failed = repository
        .fail(
            first_deployment_id,
            1,
            private_failure.into(),
            now + Duration::seconds(1),
        )
        .await?;
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert_eq!(
        repository
            .fail(
                first_deployment_id,
                1,
                private_failure.into(),
                now + Duration::seconds(1),
            )
            .await?,
        failed
    );

    let second = deployment_bundle(
        workload.clone(),
        2,
        '2',
        now + Duration::seconds(2),
        "rollout-health-2",
    )?;
    let second_operation_id = second.operation.id;
    let second_deployment_id = second.deployment.id;
    let second_revision_id = second.revision.id;
    repository.create_deployment(second).await?;
    let resolving = repository
        .mark_resolving(second_deployment_id, 1, now + Duration::seconds(3))
        .await?;
    let node_id = NodeId::new();
    let scheduled = repository
        .assign_node(
            second_deployment_id,
            resolving.aggregate_version,
            node_id,
            now + Duration::seconds(4),
        )
        .await?;
    let second_command_id = NodeCommandId::new();
    let applying = repository
        .mark_dispatched(
            second_deployment_id,
            scheduled.aggregate_version,
            second_command_id,
            now + Duration::seconds(5),
        )
        .await?;
    let verifying = repository
        .mark_verifying(
            second_deployment_id,
            applying.aggregate_version,
            now + Duration::seconds(6),
        )
        .await?;
    let (active_workload, healthy) = repository
        .activate(
            second_deployment_id,
            verifying.aggregate_version,
            false,
            now + Duration::seconds(7),
        )
        .await?;
    assert_eq!(healthy.status, DeploymentStatus::Active);
    assert_eq!(active_workload.active_revision_id, Some(second_revision_id));
    assert_eq!(
        repository
            .activate(
                second_deployment_id,
                verifying.aggregate_version,
                false,
                now + Duration::seconds(7),
            )
            .await?,
        (active_workload.clone(), healthy)
    );

    let third = deployment_bundle(
        active_workload.clone(),
        3,
        '3',
        now + Duration::seconds(8),
        "rollout-health-3",
    )?;
    let third_deployment_id = third.deployment.id;
    repository.create_deployment(third).await?;
    let resolving = repository
        .mark_resolving(third_deployment_id, 1, now + Duration::seconds(9))
        .await?;
    let scheduled = repository
        .assign_node(
            third_deployment_id,
            resolving.aggregate_version,
            node_id,
            now + Duration::seconds(10),
        )
        .await?;
    let applying = repository
        .mark_dispatched(
            third_deployment_id,
            scheduled.aggregate_version,
            NodeCommandId::new(),
            now + Duration::seconds(11),
        )
        .await?;
    let verifying = repository
        .mark_verifying(
            third_deployment_id,
            applying.aggregate_version,
            now + Duration::seconds(12),
        )
        .await?;
    let (latest_workload, retiring) = repository
        .activate(
            third_deployment_id,
            verifying.aggregate_version,
            true,
            now + Duration::seconds(13),
        )
        .await?;
    assert_eq!(retiring.status, DeploymentStatus::Retiring);
    let private_cleanup_failure = "private cleanup failure must not escape";
    let orphaned = repository
        .fail(
            third_deployment_id,
            retiring.aggregate_version,
            private_cleanup_failure.into(),
            now + Duration::seconds(14),
        )
        .await?;
    assert_eq!(orphaned.status, DeploymentStatus::Orphaned);

    let fourth = deployment_bundle(
        latest_workload.clone(),
        4,
        '4',
        now + Duration::seconds(15),
        "rollout-health-4",
    )?;
    let fourth_deployment_id = fourth.deployment.id;
    repository.create_deployment(fourth).await?;
    let resolving = repository
        .mark_resolving(fourth_deployment_id, 1, now + Duration::seconds(16))
        .await?;
    let retained_node_id = node_id;
    let scheduled = repository
        .assign_node(
            fourth_deployment_id,
            resolving.aggregate_version,
            retained_node_id,
            now + Duration::seconds(17),
        )
        .await?;
    let retained_command_id = NodeCommandId::new();
    let applying = repository
        .mark_dispatched(
            fourth_deployment_id,
            scheduled.aggregate_version,
            retained_command_id,
            now + Duration::seconds(18),
        )
        .await?;
    let private_update_failure = "private provider failure must not escape";
    assert_eq!(
        repository
            .fail(
                fourth_deployment_id,
                applying.aggregate_version,
                private_update_failure.into(),
                now + Duration::seconds(19),
            )
            .await?
            .status,
        DeploymentStatus::Failed
    );

    let fifth = deployment_bundle(
        latest_workload,
        5,
        '5',
        now + Duration::seconds(20),
        "rollout-health-5",
    )?;
    let fifth_deployment_id = fifth.deployment.id;
    repository.create_deployment(fifth).await?;
    assert_eq!(
        repository
            .cancel(fifth_deployment_id, 1, now + Duration::seconds(21))
            .await?
            .status,
        DeploymentStatus::Cancelled
    );

    let outbox = repository.outbox_events().await;
    let health_facts = outbox
        .iter()
        .filter(|event| {
            matches!(
                event.event_key.as_str(),
                "workload.deployment.failed" | "workload.deployment.healthy"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(health_facts.len(), 4);
    assert!(!serde_json::to_string(&outbox)?.contains(private_failure));
    assert!(!serde_json::to_string(&outbox)?.contains(private_cleanup_failure));
    assert!(!serde_json::to_string(&outbox)?.contains(private_update_failure));

    let failed_fact = health_facts
        .iter()
        .find(|event| {
            event.event_key == "workload.deployment.failed" && event.aggregate_version == 1
        })
        .ok_or("failed rollout-health fact")?;
    assert_eq!(failed_fact.aggregate_id, workload.id.as_uuid());
    assert_eq!(failed_fact.aggregate_version, 1);
    assert_eq!(failed_fact.schema_version, 1);
    assert_eq!(failed_fact.occurred_at, failed.updated_at);
    assert_eq!(failed_fact.correlation_id, first_operation_id.as_uuid());
    assert_eq!(failed_fact.causation_id, None);
    let failed_payload: WorkloadDeploymentHealthChanged =
        serde_json::from_value(failed_fact.payload.clone())?;
    assert_eq!(
        failed_payload.status,
        WorkloadDeploymentHealthStatus::Failed
    );
    assert_eq!(failed_payload.organization_id, workload.organization_id);
    assert_eq!(failed_payload.project_id, workload.project_id);
    assert_eq!(failed_payload.environment_id, workload.environment_id);
    assert_eq!(failed_payload.workload_id, workload.id);
    assert_eq!(failed_payload.workload_name, workload.name.as_str());
    assert_eq!(failed_payload.deployment_id, first_deployment_id);
    assert_eq!(failed_payload.revision_id, first_revision_id);
    assert_eq!(failed_payload.revision_generation, 1);
    assert_eq!(failed_payload.operation_id, first_operation_id);
    assert_eq!(
        failed_payload.failure_phase,
        Some(WorkloadDeploymentFailurePhase::Queued)
    );
    assert_eq!(
        failed_payload.availability_impact,
        Some(WorkloadDeploymentAvailabilityImpact::Unavailable)
    );
    assert_eq!(failed_payload.node_id, None);

    let retained_fact = health_facts
        .iter()
        .find(|event| {
            event.event_key == "workload.deployment.failed" && event.aggregate_version == 4
        })
        .ok_or("failed retained-revision rollout-health fact")?;
    assert_eq!(
        retained_fact.causation_id,
        Some(retained_command_id.as_uuid())
    );
    let retained_payload: WorkloadDeploymentHealthChanged =
        serde_json::from_value(retained_fact.payload.clone())?;
    assert_eq!(
        retained_payload.failure_phase,
        Some(WorkloadDeploymentFailurePhase::Applying)
    );
    assert_eq!(
        retained_payload.availability_impact,
        Some(WorkloadDeploymentAvailabilityImpact::PreviousRevisionRetained)
    );
    assert_eq!(retained_payload.node_id, Some(retained_node_id));

    let first_healthy_fact = health_facts
        .iter()
        .find(|event| {
            event.event_key == "workload.deployment.healthy" && event.aggregate_version == 2
        })
        .ok_or("healthy rollout-health fact")?;
    assert_eq!(first_healthy_fact.aggregate_id, workload.id.as_uuid());
    assert_eq!(first_healthy_fact.aggregate_version, 2);
    assert_eq!(first_healthy_fact.schema_version, 1);
    assert_eq!(
        first_healthy_fact.correlation_id,
        second_operation_id.as_uuid()
    );
    assert_eq!(
        first_healthy_fact.causation_id,
        Some(second_command_id.as_uuid())
    );
    let healthy_payload: WorkloadDeploymentHealthChanged =
        serde_json::from_value(first_healthy_fact.payload.clone())?;
    assert_eq!(
        healthy_payload.status,
        WorkloadDeploymentHealthStatus::Healthy
    );
    assert_eq!(healthy_payload.organization_id, workload.organization_id);
    assert_eq!(healthy_payload.project_id, workload.project_id);
    assert_eq!(healthy_payload.environment_id, workload.environment_id);
    assert_eq!(healthy_payload.workload_id, workload.id);
    assert_eq!(healthy_payload.workload_name, workload.name.as_str());
    assert_eq!(healthy_payload.deployment_id, second_deployment_id);
    assert_eq!(healthy_payload.failure_phase, None);
    assert_eq!(healthy_payload.availability_impact, None);
    assert_eq!(healthy_payload.node_id, Some(node_id));
    assert_eq!(healthy_payload.revision_id, second_revision_id);
    assert_eq!(healthy_payload.revision_generation, 2);
    assert_eq!(healthy_payload.operation_id, second_operation_id);

    assert_eq!(
        health_facts
            .iter()
            .filter(|event| event.event_key == "workload.deployment.healthy")
            .map(|event| event.aggregate_version)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    Ok(())
}

#[test]
fn already_selected_revision_does_not_emit_another_rollout_fact(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let workload = Workload::create(
        WorkloadId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("selected rollout revision")?,
        now,
    );
    let bundle = deployment_bundle(workload, 1, '6', now, "selected-rollout-revision")?;

    let mut previous_workload = bundle.workload.clone();
    previous_workload.activate(bundle.revision.id, now + Duration::seconds(1))?;
    let mut current_workload = previous_workload.clone();
    let mut previous_deployment = bundle.deployment.clone();
    previous_deployment.resolve(now + Duration::seconds(2))?;
    previous_deployment.schedule(NodeId::new(), now + Duration::seconds(3))?;
    previous_deployment.dispatch(NodeCommandId::new(), now + Duration::seconds(4))?;
    previous_deployment.verify(now + Duration::seconds(5))?;
    let mut current_deployment = previous_deployment.clone();
    current_deployment.activate(false, now + Duration::seconds(6))?;
    current_workload.activate(bundle.revision.id, now + Duration::seconds(6))?;

    assert!(WorkloadDeploymentHealthChanged::healthy_envelope(
        &previous_deployment,
        &current_deployment,
        &previous_workload,
        &current_workload,
        &bundle.revision,
    )?
    .is_none());

    let mut previous_failure = Deployment::create(
        DeploymentId::new(),
        current_workload.organization_id,
        current_workload.id,
        bundle.revision.id,
        OperationId::new(),
        now + Duration::seconds(7),
    );
    previous_failure.resolve(now + Duration::seconds(8))?;
    let mut failed = previous_failure.clone();
    failed.fail(
        "private same-revision failure".into(),
        now + Duration::seconds(9),
    )?;
    assert!(WorkloadDeploymentHealthChanged::failure_envelope(
        &previous_failure,
        &failed,
        &current_workload,
        &bundle.revision,
    )?
    .is_none());
    Ok(())
}
