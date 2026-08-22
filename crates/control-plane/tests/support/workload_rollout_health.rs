use super::workloads_support::request;
use a3s_cloud_contracts::NodeCommandPayload;
use a3s_cloud_control_plane::modules::fleet::domain::entities::NodeCommandDraft;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, ResourceName,
    WorkloadId,
};
use a3s_cloud_control_plane::modules::workloads::infrastructure::project_runtime_spec;
use a3s_cloud_control_plane::modules::workloads::{
    DeploymentStatus, IWorkloadRepository, PostgresWorkloadRepository, Workload,
    WorkloadDeploymentAvailabilityImpact, WorkloadDeploymentFailurePhase,
    WorkloadDeploymentHealthChanged, WorkloadDeploymentHealthStatus,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::RuntimeApplyRequest;
use chrono::{Duration, Utc};
use uuid::Uuid;

pub async fn exercise_workload_health_facts(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    project_uuid: Uuid,
    environment_uuid: Uuid,
    node_id: NodeId,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let now = Utc::now();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::from_uuid(project_uuid),
        EnvironmentId::from_uuid(environment_uuid),
        ResourceName::parse("PostgreSQL rollout health facts")?,
        now,
    );

    let first = request(workload.clone(), 1, '6', "postgres-rollout-health-1", now)?;
    let first_deployment_id = first.deployment.id;
    let first_operation_id = first.operation.id;
    repository.create_deployment(first).await?;
    let resolving = repository
        .mark_resolving(first_deployment_id, 1, now + Duration::seconds(1))
        .await?;
    let private_failure = "private PostgreSQL registry response must not escape";
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(&format!(
            "alter table outbox_events add constraint workload_health_failed_outbox_probe check (event_key <> 'workload.deployment.failed' or aggregate_id <> '{}'::uuid)",
            workload.id.as_uuid()
        ))
        .await?;
    drop(connection);
    let rejected_failure = repository
        .fail(
            first_deployment_id,
            resolving.aggregate_version,
            private_failure.into(),
            now + Duration::seconds(2),
        )
        .await;
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "alter table outbox_events drop constraint workload_health_failed_outbox_probe",
        )
        .await?;
    assert!(matches!(rejected_failure, Err(RepositoryError::Storage(_))));
    assert_eq!(
        repository
            .find_deployment(organization_id, first_deployment_id)
            .await?,
        resolving
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ",)
                    .bind(workload.id.as_uuid())
                    .append(" and event_key = 'workload.deployment.failed'"),
            )
            .await?,
        0
    );
    let failed = repository
        .fail(
            first_deployment_id,
            resolving.aggregate_version,
            private_failure.into(),
            now + Duration::seconds(2),
        )
        .await?;
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert_eq!(
        repository
            .fail(
                first_deployment_id,
                resolving.aggregate_version,
                private_failure.into(),
                now + Duration::seconds(2),
            )
            .await?,
        failed
    );

    let second = request(
        workload.clone(),
        2,
        '7',
        "postgres-rollout-health-2",
        now + Duration::seconds(3),
    )?;
    let second_deployment_id = second.deployment.id;
    let second_revision_id = second.revision.id;
    let second_operation_id = second.operation.id;
    let second_revision = second.revision.clone();
    repository.create_deployment(second).await?;
    let resolving = repository
        .mark_resolving(second_deployment_id, 1, now + Duration::seconds(4))
        .await?;
    let scheduled = repository
        .assign_node(
            second_deployment_id,
            resolving.aggregate_version,
            node_id,
            now + Duration::seconds(5),
        )
        .await?;
    let command_issued_at = now + Duration::seconds(6);
    let command_deadline = command_issued_at + Duration::minutes(5);
    let command = PostgresNodeRepository::new(executor.clone())
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id,
            aggregate_id: workload.id.as_uuid(),
            payload: NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("deployment:{second_deployment_id}:apply"),
                    deadline_at_ms: Some(u64::try_from(command_deadline.timestamp_millis())?),
                    spec: project_runtime_spec(&second_revision)?,
                }),
                resource_claim: None,
            },
            issued_at: command_issued_at,
            not_after: command_deadline,
            correlation_id: second_operation_id.as_uuid(),
        })
        .await?
        .value;
    let applying = repository
        .mark_dispatched(
            second_deployment_id,
            scheduled.aggregate_version,
            command.id,
            now + Duration::seconds(7),
        )
        .await?;
    let verifying = repository
        .mark_verifying(
            second_deployment_id,
            applying.aggregate_version,
            now + Duration::seconds(8),
        )
        .await?;
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(&format!(
            "alter table outbox_events add constraint workload_health_healthy_outbox_probe check (event_key <> 'workload.deployment.healthy' or aggregate_id <> '{}'::uuid)",
            workload.id.as_uuid()
        ))
        .await?;
    drop(connection);
    let rejected_activation = repository
        .activate(
            second_deployment_id,
            verifying.aggregate_version,
            false,
            now + Duration::seconds(9),
        )
        .await;
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "alter table outbox_events drop constraint workload_health_healthy_outbox_probe",
        )
        .await?;
    assert!(matches!(
        rejected_activation,
        Err(RepositoryError::Storage(_))
    ));
    assert_eq!(
        repository
            .find_deployment(organization_id, second_deployment_id)
            .await?,
        verifying
    );
    assert_eq!(
        repository
            .find_workload(organization_id, workload.id)
            .await?
            .active_revision_id,
        None
    );
    let (active_workload, active) = repository
        .activate(
            second_deployment_id,
            verifying.aggregate_version,
            false,
            now + Duration::seconds(9),
        )
        .await?;
    assert_eq!(active.status, DeploymentStatus::Active);
    assert_eq!(active_workload.active_revision_id, Some(second_revision_id));
    assert_eq!(
        repository
            .activate(
                second_deployment_id,
                verifying.aggregate_version,
                false,
                now + Duration::seconds(9),
            )
            .await?,
        (active_workload, active)
    );

    let facts = database
        .fetch_one_as(
            sql_query::<serde_json::Value>(
                "select coalesce(jsonb_agg(jsonb_build_object('eventKey', event_key, 'schemaVersion', schema_version, 'aggregateId', aggregate_id::text, 'aggregateVersion', aggregate_version, 'correlationId', correlation_id::text, 'causationId', causation_id::text, 'payload', payload) order by aggregate_version), '[]'::jsonb) from outbox_events where aggregate_id = ",
            )
            .bind(workload.id.as_uuid())
            .append(
                " and event_key in ('workload.deployment.failed', 'workload.deployment.healthy')",
            ),
        )
        .await?;
    let facts = facts
        .as_array()
        .ok_or("Workload health facts are not an array")?;
    assert_eq!(facts.len(), 2);
    assert!(!serde_json::to_string(facts)?.contains(private_failure));
    let failed_fact = &facts[0];
    assert_eq!(failed_fact["eventKey"], "workload.deployment.failed");
    assert_eq!(failed_fact["schemaVersion"], 1);
    assert_eq!(failed_fact["aggregateId"], workload.id.to_string());
    assert_eq!(failed_fact["aggregateVersion"], 1);
    assert_eq!(failed_fact["correlationId"], first_operation_id.to_string());
    let failed_payload: WorkloadDeploymentHealthChanged =
        serde_json::from_value(failed_fact["payload"].clone())?;
    assert_eq!(
        failed_payload.status,
        WorkloadDeploymentHealthStatus::Failed
    );
    assert_eq!(
        failed_payload.failure_phase,
        Some(WorkloadDeploymentFailurePhase::Resolving)
    );
    assert_eq!(
        failed_payload.availability_impact,
        Some(WorkloadDeploymentAvailabilityImpact::Unavailable)
    );
    assert_eq!(failed_payload.node_id, None);

    let healthy_fact = &facts[1];
    assert_eq!(healthy_fact["eventKey"], "workload.deployment.healthy");
    assert_eq!(healthy_fact["schemaVersion"], 1);
    assert_eq!(healthy_fact["aggregateId"], workload.id.to_string());
    assert_eq!(healthy_fact["aggregateVersion"], 2);
    assert_eq!(
        healthy_fact["correlationId"],
        second_operation_id.to_string()
    );
    assert_eq!(healthy_fact["causationId"], command.id.to_string());
    let healthy_payload: WorkloadDeploymentHealthChanged =
        serde_json::from_value(healthy_fact["payload"].clone())?;
    assert_eq!(
        healthy_payload.status,
        WorkloadDeploymentHealthStatus::Healthy
    );
    assert_eq!(healthy_payload.revision_id, second_revision_id);
    assert_eq!(healthy_payload.revision_generation, 2);
    assert_eq!(healthy_payload.node_id, Some(node_id));
    assert_eq!(healthy_payload.failure_phase, None);
    assert_eq!(healthy_payload.availability_impact, None);
    Ok(())
}
