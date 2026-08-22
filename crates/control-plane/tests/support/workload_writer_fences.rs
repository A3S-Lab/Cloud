use super::workloads_support::{replica_set_write, request};
use a3s_cloud_contracts::NodeCommandPayload;
use a3s_cloud_control_plane::modules::fleet::domain::entities::NodeCommandDraft;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::operations::{
    OperationRequest, OperationSubject, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError,
    ResourceName, Sha256Digest, WorkloadId,
};
use a3s_cloud_control_plane::modules::workloads::infrastructure::project_runtime_spec;
use a3s_cloud_control_plane::modules::workloads::{
    IWorkloadReplicaRetirementRepository, IWorkloadRepository, IWorkloadWriterFenceRepository,
    ManagedOwnerKind, ManagedOwnerReference, PostgresWorkloadRepository, ReplicaRetirementDispatch,
    ReplicaRuntimeFence, Workload, WorkloadControlSpec, WorkloadWriterFenceCommit,
    WorkloadWriterFenceReceipt, WorkloadWriterFenceReceiptSpec,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{RuntimeActionRequest, RuntimeApplyRequest};
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

pub async fn exercise_atomic_writer_fence_commit(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    project_uuid: Uuid,
    environment_uuid: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let project_id = ProjectId::from_uuid(project_uuid);
    let environment_id = EnvironmentId::from_uuid(environment_uuid);
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let node_id = NodeId::from_uuid(
        database
            .fetch_one_as(
                sql_query::<Uuid>("select id from nodes where organization_id = ")
                    .bind(organization_uuid)
                    .append(" and state = 'ready' order by id asc limit 1"),
            )
            .await
            .map_err(|error| format!("select writer-fence test node: {error}"))?,
    );
    let now = Utc::now();
    let owner = ManagedOwnerReference::new(
        ManagedOwnerKind::parse("durable-cell.application")?,
        Uuid::now_v7(),
        1,
        format!("sha256:{}", "a".repeat(64)),
    )?;
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("PostgreSQL writer fence")?,
        now,
    );
    let mut bundle = request(workload.clone(), 1, 'f', "postgres-writer-fence", now)?;
    bundle.control = WorkloadControlSpec::managed_replica_set(owner.clone(), 1, 1)?;
    let deployment = bundle.deployment.clone();
    let revision = bundle.revision.clone();
    repository
        .create_deployment(bundle)
        .await
        .map_err(|error| format!("create writer-fence deployment: {error}"))?;
    let apply_id = NodeCommandId::from_uuid(deployment.id.as_uuid());
    let apply_deadline = now + Duration::minutes(5);
    PostgresNodeRepository::new(executor.clone())
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: apply_id,
            node_id,
            aggregate_id: workload.id.as_uuid(),
            payload: NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("deployment:{}:apply", deployment.id),
                    deadline_at_ms: Some(u64::try_from(apply_deadline.timestamp_millis())?),
                    spec: project_runtime_spec(&revision)?,
                }),
                resource_claim: None,
            },
            issued_at: now + Duration::seconds(1),
            not_after: apply_deadline,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| format!("enqueue writer-fence RuntimeApply: {error}"))?;
    let resolving = repository
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            now + Duration::seconds(1),
        )
        .await
        .map_err(|error| format!("mark writer-fence deployment resolving: {error}"))?;
    let scheduled = repository
        .assign_node(
            deployment.id,
            resolving.aggregate_version,
            node_id,
            now + Duration::seconds(2),
        )
        .await
        .map_err(|error| format!("assign writer-fence deployment node: {error}"))?;
    repository
        .mark_dispatched(
            deployment.id,
            scheduled.aggregate_version,
            apply_id,
            now + Duration::seconds(3),
        )
        .await
        .map_err(|error| format!("mark writer-fence deployment dispatched: {error}"))?;
    let control = repository
        .find_workload_control(organization_id, workload.id)
        .await
        .map_err(|error| format!("load writer-fence Workload control: {error}"))?;
    repository
        .reconfigure_replica_set(replica_set_write(
            &control,
            0,
            "postgres-writer-fence-stop",
            now + Duration::seconds(4),
        )?)
        .await
        .map_err(|error| format!("retire writer-fence replica set: {error}"))?;
    let target = repository
        .pending_replica_retirements(100)
        .await
        .map_err(|error| format!("list writer-fence retirement targets: {error}"))?
        .into_iter()
        .find(|target| target.replica.workload_id == workload.id)
        .ok_or("writer-fence retirement target")?;
    let binding = target
        .replica_binding
        .as_ref()
        .ok_or("writer-fence replica binding")?;
    let command_id = NodeCommandId::new();
    let removal_deadline = now + Duration::minutes(10);
    let command = PostgresNodeRepository::new(executor.clone())
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id,
            aggregate_id: target.replica.id.as_uuid(),
            payload: NodeCommandPayload::RuntimeRemove {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: format!("replica-retirement:{command_id}:remove"),
                    unit_id: binding.runtime_unit_id.clone(),
                    generation: binding.runtime_generation,
                    deadline_at_ms: Some(u64::try_from(removal_deadline.timestamp_millis())?),
                },
            },
            issued_at: now + Duration::seconds(5),
            not_after: removal_deadline,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| format!("enqueue writer-fence RuntimeRemove: {error}"))?
        .value;
    let dispatched = repository
        .dispatch_replica_retirement(ReplicaRetirementDispatch {
            organization_id,
            workload_id: workload.id,
            replica_id: target.replica.id,
            replica_generation: target.replica.generation,
            expected_replica_version: target.replica.aggregate_version,
            command_id,
            dispatched_at: command.issued_at,
        })
        .await
        .map_err(|error| format!("dispatch writer-fence retirement: {error}"))?;
    let fenced_at = now + Duration::seconds(6);
    let operation_id = OperationId::new();
    let receipt = WorkloadWriterFenceReceipt::issue(WorkloadWriterFenceReceiptSpec {
        organization_id,
        project_id,
        environment_id,
        workload_id: workload.id,
        workload_revision_id: revision.id,
        workload_revision_generation: revision.generation,
        replica_id: target.replica.id,
        replica_ordinal: target.replica.ordinal,
        writer_epoch: target.replica.generation,
        member_id: target.member.id,
        placement_generation: target.member.placement_generation,
        managed_owner: owner,
        node_id,
        runtime_unit_id: binding.runtime_unit_id.clone(),
        command_id,
        command_payload_digest: Sha256Digest::parse(command.payload_digest()?)?,
        acknowledgement_digest: Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))?,
        continuation_operation_id: operation_id,
        fenced_at,
    })?;
    let fenced_at = receipt.spec().fenced_at;
    let commit = WorkloadWriterFenceCommit {
        operation: OperationRequest::new(
            operation_id,
            organization_id,
            OperationSubject::new("workload", workload.id.as_uuid())?,
            WorkflowIdentity::new("cloud.test.writer-fence", "1")?,
            json!({ "writerFenceReceiptDigest": receipt.digest() }),
            fenced_at,
        ),
        receipt,
    };
    let fence = ReplicaRuntimeFence {
        organization_id,
        workload_id: workload.id,
        replica_id: target.replica.id,
        replica_generation: target.replica.generation,
        expected_replica_version: dispatched.aggregate_version,
        command_id,
        fenced_at,
    };

    database
        .execute(sql_query::<()>(
            "create function fail_test_writer_fence_receipt_insert() returns trigger language plpgsql as $$ begin raise exception 'injected writer-fence receipt failure'; end $$",
        ))
        .await?;
    database
        .execute(sql_query::<()>(
            "create trigger fail_test_writer_fence_receipt before insert on workload_writer_fence_receipts for each row execute function fail_test_writer_fence_receipt_insert()",
        ))
        .await?;
    assert!(matches!(
        repository
            .record_replica_runtime_fenced(fence, Some(commit.clone()))
            .await,
        Err(RepositoryError::Storage(_))
    ));
    let unchanged = repository
        .find_workload_replica(organization_id, workload.id, target.replica.id)
        .await
        .map_err(|error| format!("reload rolled-back writer-fence replica: {error}"))?;
    assert_eq!(unchanged.aggregate_version, dispatched.aggregate_version);
    assert_eq!(unchanged.runtime_fenced_at, None);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from operation_requests where operation_id = ",)
                    .bind(operation_id.as_uuid()),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from workload_writer_fence_receipts where workload_id = ",
                )
                .bind(workload.id.as_uuid()),
            )
            .await?,
        0
    );
    database
        .execute(sql_query::<()>(
            "drop trigger fail_test_writer_fence_receipt on workload_writer_fence_receipts",
        ))
        .await?;
    database
        .execute(sql_query::<()>(
            "drop function fail_test_writer_fence_receipt_insert()",
        ))
        .await?;

    let fenced = repository
        .record_replica_runtime_fenced(fence, Some(commit.clone()))
        .await
        .map_err(|error| format!("commit writer-fence receipt: {error}"))?;
    assert_eq!(fenced.runtime_fenced_at, Some(fenced_at));
    assert_eq!(
        repository
            .latest_writer_fence(organization_id, workload.id)
            .await
            .map_err(|error| format!("reload writer-fence receipt: {error}"))?,
        Some(commit.receipt.clone())
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from operation_requests request join workload_writer_fence_receipts receipt on receipt.continuation_operation_id = request.operation_id where receipt.workload_id = ",
                )
                .bind(workload.id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        repository
            .record_replica_runtime_fenced(fence, Some(commit))
            .await
            .map_err(|error| format!("replay writer-fence receipt: {error}"))?,
        fenced
    );
    Ok(())
}
