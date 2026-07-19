use a3s_cloud_control_plane::modules::operations::{
    OperationRequest, OperationSubject, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OperationId,
    OrganizationId, ProjectId, RepositoryError, ResourceName, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentRequested, DeploymentStatus, HttpHealthCheck,
    IWorkloadRepository, OciArtifact, PostgresWorkloadRepository, ServicePort, ServiceProcess,
    ServiceResources, ServiceTemplate, Workload, WorkloadRevision,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Timelike, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

pub struct WorkloadFixture {
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
}

pub async fn exercise_workloads(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    project_uuid: Uuid,
    environment_uuid: Uuid,
) -> Result<WorkloadFixture, Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let project_id = ProjectId::from_uuid(project_uuid);
    let environment_id = EnvironmentId::from_uuid(environment_uuid);
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 789)
        .expect("sub-microsecond workload timestamp");
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("HTTP fixture")?,
        now,
    );
    let first_request = request(workload, 1, 'a', "deploy-http-fixture", now)?;
    let first_deployment_id = first_request.deployment.id;
    let first_revision_id = first_request.revision.id;
    let first_operation_id = first_request.operation.id;
    let (first, replay) = tokio::join!(
        repository.create_deployment(first_request.clone()),
        repository.create_deployment(first_request.clone())
    );
    let first = first?;
    let replay = replay?;
    assert_ne!(first.replayed, replay.replayed);
    assert_eq!(first.deployment.id, replay.deployment.id);
    assert_eq!(
        first
            .revision
            .resolved_template()
            .expect("stored revision is resolved")
            .artifact
            .digest,
        digest('a')
    );
    assert!(matches!(
        repository
            .find_workload(OrganizationId::new(), first.workload.id)
            .await,
        Err(RepositoryError::NotFound)
    ));

    let mut changed_idempotency = first_request.clone();
    changed_idempotency.idempotency = IdempotencyRequest::new(
        "workload.deploy",
        "deploy-http-fixture",
        b"different canonical request",
    )?;
    assert!(matches!(
        repository.create_deployment(changed_idempotency).await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workloads where id = ")
                    .bind(first.workload.id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workload_revisions where id = ")
                    .bind(first_revision_id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from operation_requests where operation_id = ")
                    .bind(first_operation_id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ")
                    .bind(first_deployment_id.as_uuid()),
            )
            .await?,
        1
    );

    let node_uuid = database
        .fetch_one_as(
            sql_query::<Uuid>("select id from nodes where organization_id = ")
                .bind(organization_uuid)
                .append(" order by id asc limit 1"),
        )
        .await?;
    let node_id = NodeId::from_uuid(node_uuid);
    let command_id = NodeCommandId::new();
    let next_sequence = database
        .fetch_one_as(
            sql_query::<i64>(
                "select coalesce(max(sequence), 0) + 1 from node_commands where node_id = ",
            )
            .bind(node_uuid),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into node_commands (id, node_id, sequence, aggregate_id, generation, command_kind, payload_schema, payload_digest, payload, issued_at, not_after, correlation_id) values (",
            )
            .bind(command_id.as_uuid())
            .append(", ")
            .bind(node_uuid)
            .append(", ")
            .bind(next_sequence)
            .append(", ")
            .bind(first_deployment_id.as_uuid())
            .append(", 1, 'runtime_apply', 'a3s.runtime.apply-request.v1', ")
            .bind(format!("sha256:{}", "e".repeat(64)))
            .append(", ")
            .bind(json!({"fixture": "workload persistence"}))
            .append(", ")
            .bind(now + Duration::seconds(2))
            .append(", ")
            .bind(now + Duration::minutes(2))
            .append(", ")
            .bind(Uuid::now_v7())
            .append(")"),
        )
        .await?;

    let resolving = repository
        .mark_resolving(first_deployment_id, 1, now + Duration::seconds(1))
        .await?;
    assert_eq!(resolving.status, DeploymentStatus::Resolving);
    assert_eq!(resolving.updated_at.nanosecond() % 1_000, 0);
    assert_eq!(
        repository
            .mark_resolving(first_deployment_id, 1, now + Duration::seconds(1))
            .await?,
        resolving
    );
    let scheduled = repository
        .assign_node(
            first_deployment_id,
            resolving.aggregate_version,
            node_id,
            now + Duration::seconds(2),
        )
        .await?;
    assert!(matches!(
        repository
            .assign_node(
                first_deployment_id,
                resolving.aggregate_version,
                NodeId::new(),
                now + Duration::seconds(2),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let applying = repository
        .mark_dispatched(
            first_deployment_id,
            scheduled.aggregate_version,
            command_id,
            now + Duration::seconds(3),
        )
        .await?;
    let verifying = repository
        .mark_verifying(
            first_deployment_id,
            applying.aggregate_version,
            now + Duration::seconds(4),
        )
        .await?;
    let (active_workload, active) = repository
        .activate(
            first_deployment_id,
            verifying.aggregate_version,
            now + Duration::seconds(5),
        )
        .await?;
    assert_eq!(active.status, DeploymentStatus::Active);
    assert_eq!(active_workload.active_revision_id, Some(first_revision_id));
    assert_eq!(
        repository
            .activate(
                first_deployment_id,
                verifying.aggregate_version,
                now + Duration::seconds(5),
            )
            .await?,
        (active_workload.clone(), active.clone())
    );

    let second_request = request(
        active_workload.clone(),
        2,
        'b',
        "deploy-http-fixture-v2",
        now + Duration::seconds(6),
    )?;
    let second_id = second_request.deployment.id;
    repository.create_deployment(second_request).await?;
    let second = repository
        .mark_resolving(second_id, 1, now + Duration::seconds(7))
        .await?;
    let failed = repository
        .fail(
            second_id,
            second.aggregate_version,
            "health check never stabilized".into(),
            now + Duration::seconds(8),
        )
        .await?;
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert_eq!(
        repository
            .find_workload(organization_id, active_workload.id)
            .await?
            .active_revision_id,
        Some(first_revision_id)
    );

    let third_request = request(
        active_workload.clone(),
        3,
        'c',
        "deploy-http-fixture-v3",
        now + Duration::seconds(9),
    )?;
    let third_id = third_request.deployment.id;
    repository.create_deployment(third_request).await?;
    assert_eq!(
        repository
            .cancel(third_id, 1, now + Duration::seconds(10))
            .await?
            .status,
        DeploymentStatus::Cancelled
    );
    assert_eq!(
        repository
            .list_deployments(organization_id, active_workload.id)
            .await?
            .len(),
        3
    );

    let rolled_back_workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("Atomic rollback fixture")?,
        now + Duration::seconds(11),
    );
    let mut rolled_back = request(
        rolled_back_workload,
        1,
        'd',
        "deploy-atomic-rollback",
        now + Duration::seconds(11),
    )?;
    let rolled_back_workload_id = rolled_back.workload.id;
    let rolled_back_operation_id = rolled_back.operation.id;
    rolled_back.event.schema_version = 0;
    assert!(matches!(
        repository.create_deployment(rolled_back).await,
        Err(RepositoryError::Storage(_))
    ));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workloads where id = ")
                    .bind(rolled_back_workload_id.as_uuid()),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from operation_requests where operation_id = ")
                    .bind(rolled_back_operation_id.as_uuid()),
            )
            .await?,
        0
    );
    Ok(WorkloadFixture {
        workload_id: active_workload.id,
        revision_id: first_revision_id,
        node_id,
    })
}

fn request(
    workload: Workload,
    generation: u64,
    digest_character: char,
    idempotency_key: &str,
    requested_at: chrono::DateTime<Utc>,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        generation,
        template(digest_character),
        requested_at,
    )?;
    let deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        requested_at,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        workload.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new("cloud.deployment", "1")?,
        json!({
            "deploymentId": deployment.id,
            "generation": generation,
            "revisionId": revision.id,
        }),
        requested_at,
    );
    let event = DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?;
    let canonical = serde_json::to_vec(&json!({
        "workloadId": workload.id,
        "generation": generation,
        "templateDigest": revision.template_digest,
    }))?;
    Ok(CreateDeploymentBundle {
        workload,
        revision,
        deployment,
        operation,
        idempotency: IdempotencyRequest::new("workload.deploy", idempotency_key, &canonical)?,
        event,
    })
}

fn template(digest_character: char) -> ServiceTemplate {
    let digest = digest(digest_character);
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/a3s-cloud/http-fixture@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/http-fixture".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        resources: ServiceResources {
            cpu_millis: 250,
            memory_bytes: 64 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5_000,
        },
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
