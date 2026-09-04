use super::super::commands::{
    StartDurableCellApplication, StartDurableCellApplicationHandler, StopDurableCellApplication,
    StopDurableCellApplicationHandler,
};
use super::super::writer_fence::DurableCellWriterFenceAdapter;
use super::*;
use crate::modules::data::{
    ObjectNamespaceCredentialBindingSpec, ObjectNamespaceRetentionPolicySpec,
    SealObjectNamespaceOperationInput,
};
use crate::modules::durable_cells::domain::{
    CreateDurableCellApplicationWrite, DurableCellApplication, DurableCellApplicationChanged,
    DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
    DurableCellApplicationRevision, DurableCellClassSpec, DurableCellRollbackPolicy,
    DurableCellStateSchema, RequestDurableCellApplicationStateWrite,
    ReviseDurableCellApplicationWrite,
};
use crate::modules::durable_cells::infrastructure::{
    DataDurableCellStorageAdapter, FleetDurableCellNodePoolAdapter,
    InMemoryDurableCellApplicationRepository, InMemoryDurableCellDeploymentRepository,
    OperationsDurableCellOperationAdapter, SecretsDurableCellBindingAdapter,
    WorkloadsDurableCellWorkloadAdapter,
};
use crate::modules::durable_cells::IDurableCellOperationPort;
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::operations::InMemoryOperationRepository;
use crate::modules::secrets::application::exact_secret_version_access;
use crate::modules::secrets::domain::{
    CreateSecretWrite, EncryptedSecretValue, ISecretRepository, Secret, SecretChanged,
};
use crate::modules::secrets::infrastructure::InMemorySecretRepository;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, DurableCellApplicationRevisionId, NodeCommandId, NodeId,
    ResourceName, SecretId, SecretVersionReference,
};
use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
use crate::modules::workloads::{
    HttpHealthCheck, IWorkloadReplicaRetirementRepository, IWorkloadRepository,
    IWorkloadWriterFenceAdapter, IWorkloadWriterFenceRepository, OciArtifact,
    ReplicaRetirementCompletion, ReplicaRetirementDispatch, ReplicaRuntimeFence, SecretBinding,
    SecretBindingTarget, ServicePort, ServiceResources, WorkloadDeploymentAvailabilityImpact,
    WorkloadDeploymentFailurePhase, WorkloadDeploymentHealthChanged,
    WorkloadDeploymentHealthStatus, WorkloadReplicaLifecycle,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_runtime::contract::{RuntimeActionRequest, RuntimeRemoval};
use serde::Serialize;

#[tokio::test]
async fn persisted_intents_recover_through_the_existing_managed_workload_lifecycle() {
    let now = Utc::now() - chrono::Duration::seconds(5);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor_principal_id = PrincipalId::new();
    let profile = service_profile();
    let applications = Arc::new(InMemoryDurableCellApplicationRepository::new());
    let record = application_record(
        organization_id,
        project_id,
        environment_id,
        actor_principal_id,
        &profile,
        now,
    );
    let application_request_id = Uuid::now_v7();
    applications
        .create(CreateDurableCellApplicationWrite {
            event: DurableCellApplicationChanged::created(
                &record.application,
                &record.revision,
                application_request_id,
            )
            .expect("application event"),
            actor_principal_id,
            request_id: application_request_id,
            idempotency: IdempotencyRequest::new(
                "durable-cell-deployment-test/application",
                "create",
                record.application.id.as_uuid().as_bytes(),
            )
            .expect("application idempotency"),
            record: record.clone(),
        })
        .await
        .expect("store application");

    let projection =
        DurableCellProjectionIdentity::for_current_revision(&record.application, &record.revision)
            .expect("projection");
    let secrets = Arc::new(InMemorySecretRepository::new());
    let access_key_id = store_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "S0 access key",
        now,
    )
    .await;
    let secret_access_key = store_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "S0 secret key",
        now,
    )
    .await;
    let storage_provider_profile =
        ObjectNamespaceProviderProfile::parse_acl(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/s0.1/object-namespace-provider-profile.acl"
        )))
        .expect("storage provider profile");
    let storage_credentials =
        ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
            organization_id,
            project_id,
            environment_id,
            namespace_id: projection.storage_namespace_id,
            generation: 1,
            provider_profile_digest: storage_provider_profile.digest().clone(),
            access_key_id,
            secret_access_key,
            session_token: None,
        })
        .expect("storage credentials");
    let retention_policy =
        ObjectNamespaceRetentionPolicy::from_spec(ObjectNamespaceRetentionPolicySpec {
            minimum_sealed_recovery_points: 2,
            maximum_sealed_recovery_points: 24,
            maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
            deletion_grace_period_seconds: 24 * 60 * 60,
        })
        .expect("retention policy");
    let command = DeployDurableCellApplication {
        organization_id,
        project_id,
        environment_id,
        application_id: record.application.id,
        application_revision_id: record.revision.id,
        service_profile_acl: profile.canonical_acl().into(),
        storage_provider_profile_acl: Some(storage_provider_profile.canonical_acl().into()),
        workload_template: service_template(
            &profile,
            &storage_provider_profile,
            projection.storage_namespace_id,
            access_key_id,
            secret_access_key,
        ),
        storage_credentials,
        retention_policy,
        node_pool_id: None,
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "deploy-counters".into(),
        request_id: Uuid::now_v7(),
    };
    let deployments = Arc::new(InMemoryDurableCellDeploymentRepository::new());
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let workload_port: Arc<dyn IDurableCellWorkloadPort> =
        Arc::new(WorkloadsDurableCellWorkloadAdapter::new(
            applications.clone(),
            workloads.clone(),
            workloads.clone(),
        ));
    let node_pools = Arc::new(InMemoryNodeRepository::new());
    let node_pool_port: Arc<dyn IDurableCellNodePoolPort> =
        Arc::new(FleetDurableCellNodePoolAdapter::new(node_pools.clone()));
    let secret_port: Arc<dyn ISecretRepository> = secrets.clone();
    let secret_binding_port: Arc<dyn IDurableCellSecretBindingPort> =
        Arc::new(SecretsDurableCellBindingAdapter::new(
            exact_secret_version_access(Arc::clone(&secret_port)),
        ));
    let storage_port: Arc<dyn IDurableCellStoragePort> =
        Arc::new(DataDurableCellStorageAdapter::new(Arc::clone(&secret_port)));

    let mut missing_storage_process = command.clone();
    missing_storage_process.workload_template.process.args = vec![
        "--listen".into(),
        "0.0.0.0:8080".into(),
        "--internal-listen".into(),
        "0.0.0.0:8081".into(),
    ];
    assert!(PreparedDeployment::new(&missing_storage_process).is_err());
    missing_storage_process.storage_provider_profile_acl = None;
    PreparedDeployment::new(&missing_storage_process)
        .expect("legacy deployment remains outside the pinned celld/S0 adapter");

    // Persist the exact intent without invoking Workloads, modeling a
    // process death at the cross-owner boundary.
    let prepared = PreparedDeployment::new(&command).expect("prepared deployment");
    let correlation_idempotency = prepared
        .idempotency(&command.idempotency_key)
        .expect("correlation idempotency");
    let workload_idempotency = prepared
        .workload_idempotency(&command.idempotency_key)
        .expect("Workload idempotency");
    assert!(correlation_idempotency.scope.starts_with(&format!(
        "organizations/{organization_id}/durable-cell-applications/{}/revisions/{}/",
        record.application.id, record.revision.id,
    )));
    assert_ne!(correlation_idempotency.scope, workload_idempotency.scope);
    admit_external_bindings(
        storage_port.as_ref(),
        secret_binding_port.as_ref(),
        node_pool_port.as_ref(),
        &command,
    )
    .await
    .expect("external admission");
    let correlation = prepare_correlation(workload_port.as_ref(), &record, &command, &prepared)
        .await
        .expect("correlation");
    deployments
        .create(CreateDurableCellDeploymentWrite {
            deployment: correlation.clone(),
            idempotency: correlation_idempotency,
        })
        .await
        .expect("persist correlation");
    assert!(matches!(
        workloads
            .find_workload(organization_id, projection.workload_id)
            .await,
        Err(RepositoryError::NotFound)
    ));

    let handler = DeployDurableCellApplicationHandler::new(
        applications.clone(),
        deployments.clone(),
        workload_port.clone(),
        Arc::clone(&storage_port),
        secret_binding_port,
        node_pool_port,
    );
    let recovered = handler
        .execute(command.clone(), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("command framework")
        .expect("recover deployment");
    assert!(recovered.replayed);
    assert_eq!(recovered.correlation, correlation);
    assert_eq!(recovered.workload.workload_id, projection.workload_id);
    assert_eq!(
        recovered.workload.revision_id,
        projection.workload_revision_id
    );
    assert_eq!(recovered.workload.deployment_id, projection.deployment_id);
    assert_eq!(recovered.workload.operation_id, projection.operation_id);
    let control = workloads
        .find_workload_control(organization_id, projection.workload_id)
        .await
        .expect("managed control");
    let owner = control.spec.managed_owner.expect("managed owner");
    assert_eq!(owner.kind().as_str(), "durable-cell.application");
    assert_eq!(owner.owner_id(), record.application.id.as_uuid());
    assert_eq!(workloads.outbox_events().await.len(), 1);

    let replay = handler
        .execute(command.clone(), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("command framework")
        .expect("exact replay");
    assert!(replay.replayed);
    assert_eq!(replay.correlation, recovered.correlation);
    assert_eq!(replay.workload, recovered.workload);
    assert_eq!(workloads.outbox_events().await.len(), 1);

    workloads
        .fail(
            projection.deployment_id,
            recovered.workload.deployment_aggregate_version,
            "complete the first fixture generation".into(),
            Utc::now(),
        )
        .await
        .expect("terminal first deployment");
    let mut second_definition = record.revision.definition.spec().clone();
    second_definition.build_run_id = BuildRunId::new();
    second_definition.bundle_digest = digest('b');
    let second_revision = DurableCellApplicationRevision::successor(
        &record.revision,
        DurableCellApplicationRevisionId::new(),
        DurableCellApplicationDefinition::from_spec(second_definition).expect("second definition"),
        actor_principal_id,
        now + chrono::Duration::seconds(1),
    )
    .expect("second revision");
    let second_application = record
        .application
        .advance(record.application.aggregate_version, &second_revision)
        .expect("second application head");
    let second_record =
        DurableCellApplicationRecord::new(second_application.clone(), second_revision.clone())
            .expect("second record");
    store_application_revision(
        applications.as_ref(),
        &record,
        second_record.clone(),
        actor_principal_id,
        "revise-to-undeployed-two",
    )
    .await;

    let mut third_definition = second_revision.definition.spec().clone();
    third_definition.build_run_id = BuildRunId::new();
    third_definition.bundle_digest = digest('e');
    let third_revision = DurableCellApplicationRevision::successor(
        &second_revision,
        DurableCellApplicationRevisionId::new(),
        DurableCellApplicationDefinition::from_spec(third_definition).expect("third definition"),
        actor_principal_id,
        now + chrono::Duration::seconds(2),
    )
    .expect("third revision");
    let third_application = second_application
        .advance(second_application.aggregate_version, &third_revision)
        .expect("third application head");
    let third_record = DurableCellApplicationRecord::new(third_application, third_revision.clone())
        .expect("third record");
    store_application_revision(
        applications.as_ref(),
        &second_record,
        third_record,
        actor_principal_id,
        "revise-to-deployed-three",
    )
    .await;

    let third = handler
        .execute(
            DeployDurableCellApplication {
                application_revision_id: third_revision.id,
                idempotency_key: "deploy-counters-third-revision".into(),
                request_id: Uuid::now_v7(),
                ..command.clone()
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("command framework")
        .expect("deploy third application revision");
    assert!(!third.replayed);
    assert_eq!(third.correlation.projection.application_revision_number, 3);
    assert_eq!(third.correlation.provider.workload_generation, 2);
    let rollout_outbox = workloads.outbox_events().await;
    assert_eq!(rollout_outbox.len(), 3);
    let failed_fact = rollout_outbox
        .iter()
        .find(|event| event.event_key == "workload.deployment.failed")
        .expect("failed managed Workload rollout fact");
    assert_eq!(failed_fact.aggregate_id, projection.workload_id.as_uuid());
    assert_eq!(failed_fact.aggregate_version, 1);
    assert_eq!(
        failed_fact.correlation_id,
        projection.operation_id.as_uuid()
    );
    let failed_payload: WorkloadDeploymentHealthChanged =
        serde_json::from_value(failed_fact.payload.clone()).expect("typed rollout failure fact");
    assert_eq!(
        failed_payload.status,
        WorkloadDeploymentHealthStatus::Failed
    );
    assert_eq!(
        failed_payload.failure_phase,
        Some(WorkloadDeploymentFailurePhase::Queued)
    );
    assert_eq!(
        failed_payload.availability_impact,
        Some(WorkloadDeploymentAvailabilityImpact::Unavailable)
    );
    assert!(!serde_json::to_string(&rollout_outbox)
        .expect("rollout Outbox JSON")
        .contains("complete the first fixture generation"));
    let advanced_control = workloads
        .find_workload_control(organization_id, projection.workload_id)
        .await
        .expect("advanced managed control");
    let advanced_owner = advanced_control
        .spec
        .managed_owner
        .as_ref()
        .expect("advanced owner");
    assert_eq!(advanced_owner.owner_generation(), 3);
    assert_eq!(advanced_control.spec.placement_policy.generation(), 2);
    assert_eq!(advanced_control.aggregate_version, 2);

    let placed_at = canonical_timestamp(Utc::now());
    let resolving = workloads
        .mark_resolving(
            third.workload.deployment_id,
            third.workload.deployment_aggregate_version,
            placed_at,
        )
        .await
        .expect("resolve third provider deployment");
    let provider_node_id = NodeId::new();
    let scheduled = workloads
        .assign_node(
            resolving.id,
            resolving.aggregate_version,
            provider_node_id,
            placed_at,
        )
        .await
        .expect("schedule third provider deployment");
    workloads
        .mark_dispatched(
            scheduled.id,
            scheduled.aggregate_version,
            NodeCommandId::new(),
            placed_at,
        )
        .await
        .expect("dispatch third provider deployment");

    // Model process death after the Durable Cell desired-state transaction
    // commits but before the Workloads-owned replica transaction begins.
    let stop_key = "stop-deployed-counters";
    let stop_request_id = Uuid::now_v7();
    let current_application = applications
        .find(
            organization_id,
            project_id,
            environment_id,
            record.application.id,
        )
        .await
        .expect("current application query")
        .expect("current application");
    assert_eq!(current_application.aggregate_version, 3);
    let stopped_application = current_application
        .request_state(
            current_application.aggregate_version,
            DurableCellApplicationDesiredState::Stopped,
            placed_at + chrono::Duration::milliseconds(1),
        )
        .expect("stopped application intent");
    let stopped_record =
        DurableCellApplicationRecord::new(stopped_application.clone(), third_revision.clone())
            .expect("stopped record");
    let stop_idempotency = state_idempotency(
        &stopped_record,
        current_application.aggregate_version,
        stop_key,
    );
    applications
        .request_state(RequestDurableCellApplicationStateWrite {
            event: DurableCellApplicationChanged::state_requested(
                &stopped_application,
                &third_revision,
                stop_request_id,
            )
            .expect("stop event"),
            record: stopped_record.clone(),
            expected_version: current_application.aggregate_version,
            actor_principal_id,
            request_id: stop_request_id,
            idempotency: stop_idempotency,
        })
        .await
        .expect("persist stopped intent");
    assert_eq!(
        workloads
            .find_workload_control(organization_id, projection.workload_id)
            .await
            .expect("control before recovery")
            .spec
            .placement_policy
            .desired_replicas(),
        1
    );
    assert_eq!(workloads.outbox_events().await.len(), 3);

    let stop_command = StopDurableCellApplication {
        organization_id,
        project_id,
        environment_id,
        application_id: record.application.id,
        expected_version: current_application.aggregate_version,
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: stop_key.into(),
        request_id: stop_request_id,
    };
    let stop_handler =
        StopDurableCellApplicationHandler::new(applications.clone(), workload_port.clone());
    let recovered_stop = stop_handler
        .execute(stop_command.clone(), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("command framework")
        .expect("recover stopped replica intent");
    assert!(recovered_stop.replayed);
    assert_eq!(recovered_stop.record, stopped_record);
    let stopped_control = workloads
        .find_workload_control(organization_id, projection.workload_id)
        .await
        .expect("stopped control");
    assert_eq!(stopped_control.spec.placement_policy.desired_replicas(), 0);
    let stopped_replicas = workloads
        .list_workload_replicas(organization_id, projection.workload_id)
        .await
        .expect("stopped replicas");
    assert_eq!(stopped_replicas.len(), 1);
    assert_eq!(
        stopped_replicas[0].lifecycle,
        WorkloadReplicaLifecycle::Retiring
    );
    assert_eq!(workloads.outbox_events().await.len(), 4);
    assert!(
        stop_handler
            .execute(stop_command, CqrsContext::new(ModuleRef::new()),)
            .await
            .expect("command framework")
            .expect("exact stop replay")
            .replayed
    );
    assert_eq!(workloads.outbox_events().await.len(), 4);

    // The existing Workloads retirement authority performs cleanup. The
    // Durable Cell adapter admits that exact RuntimeRemove receipt and
    // contributes the S0 continuation to the same Runtime-fence commit.
    let mut retirements = workloads
        .pending_replica_retirements(10)
        .await
        .expect("pending retirement");
    assert_eq!(retirements.len(), 1);
    let retirement = retirements.remove(0);
    assert_eq!(retirement.member.node_id, Some(provider_node_id));
    assert!(retirement
        .deployment
        .as_ref()
        .is_some_and(|deployment| deployment.command_id.is_some()));
    let replica_binding = retirement
        .replica_binding
        .as_ref()
        .expect("retiring provider replica binding");
    let removal_command_id = NodeCommandId::new();
    let removal_issued_at = placed_at + chrono::Duration::milliseconds(2);
    let removal_completed_at = placed_at + chrono::Duration::milliseconds(3);
    let removal_request = RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("replica-retirement:{removal_command_id}:remove"),
        unit_id: replica_binding.runtime_unit_id.clone(),
        generation: replica_binding.runtime_generation,
        deadline_at_ms: Some(
            u64::try_from((removal_completed_at + chrono::Duration::minutes(1)).timestamp_millis())
                .expect("positive Runtime deadline"),
        ),
    };
    let removal_command = NodeCommand::issue(
        NodeCommandDraft {
            proposed_command_id: removal_command_id,
            node_id: provider_node_id,
            aggregate_id: retirement.replica.id.as_uuid(),
            payload: NodeCommandPayload::RuntimeRemove {
                request: removal_request.clone(),
            },
            issued_at: removal_issued_at,
            not_after: removal_completed_at + chrono::Duration::minutes(2),
            correlation_id: third.correlation.projection.operation_id.as_uuid(),
        },
        1,
    )
    .expect("RuntimeRemove command");
    let removal_acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: removal_command.id.as_uuid(),
        lease_id: Uuid::now_v7(),
        node_id: provider_node_id.as_uuid(),
        sequence: removal_command.sequence,
        payload_digest: removal_command
            .payload_digest()
            .expect("RuntimeRemove digest"),
        completed_at: removal_completed_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeRemoved {
                removal: RuntimeRemoval {
                    schema: RuntimeRemoval::SCHEMA.into(),
                    request_id: removal_request.request_id.clone(),
                    unit_id: removal_request.unit_id.clone(),
                    generation: removal_request.generation,
                    removed_at_ms: u64::try_from(removal_completed_at.timestamp_millis())
                        .expect("positive Runtime removal time"),
                    already_absent: false,
                },
            }),
        },
    };
    let dispatched_replica = workloads
        .dispatch_replica_retirement(ReplicaRetirementDispatch {
            organization_id,
            workload_id: projection.workload_id,
            replica_id: retirement.replica.id,
            replica_generation: retirement.replica.generation,
            expected_replica_version: retirement.replica.aggregate_version,
            command_id: removal_command.id,
            dispatched_at: removal_issued_at,
        })
        .await
        .expect("dispatch provider RuntimeRemove");
    let writer_fence = DurableCellWriterFenceAdapter::new(
        applications.clone(),
        deployments.clone(),
        workload_port.clone(),
        Arc::new(OperationsDurableCellOperationAdapter::new(Arc::new(
            InMemoryOperationRepository::new(),
        ))) as Arc<dyn IDurableCellOperationPort>,
        Arc::clone(&storage_port),
    )
    .prepare_replica_retirement(&retirement, &removal_command, &removal_acknowledgement)
    .await
    .expect("prepare Durable Cell writer fence")
    .expect("stopped Durable Cell continuation");
    let seal_input: SealObjectNamespaceOperationInput =
        serde_json::from_value(writer_fence.operation.input.clone()).expect("namespace seal input");
    assert_eq!(seal_input.writer_epoch, retirement.replica.generation);
    assert!(seal_input.previous_recovery_point.is_none());
    assert_eq!(
        seal_input.writer_fence_receipt_digest,
        *writer_fence.receipt.digest()
    );
    let fenced = workloads
        .record_replica_runtime_fenced(
            ReplicaRuntimeFence {
                organization_id,
                workload_id: projection.workload_id,
                replica_id: retirement.replica.id,
                replica_generation: retirement.replica.generation,
                expected_replica_version: dispatched_replica.aggregate_version,
                command_id: removal_command.id,
                fenced_at: removal_completed_at,
            },
            Some(writer_fence.clone()),
        )
        .await
        .expect("commit Durable Cell writer fence");
    let stored_receipt = workloads
        .latest_writer_fence(organization_id, projection.workload_id)
        .await
        .expect("load writer fence")
        .expect("stored writer fence");
    assert_eq!(stored_receipt, writer_fence.receipt);
    assert_eq!(
        workloads
            .writer_fence_operation(stored_receipt.spec().continuation_operation_id)
            .await,
        Some(writer_fence.operation)
    );

    // Once the exact cleanup is terminal, start reactivates the same
    // replica; Durable Cells does not create another rollout authority.
    let retired = workloads
        .complete_replica_retirement(ReplicaRetirementCompletion {
            organization_id,
            workload_id: projection.workload_id,
            replica_id: retirement.replica.id,
            replica_generation: retirement.replica.generation,
            expected_replica_version: fenced.aggregate_version,
            member_id: retirement.member.id,
            expected_member_version: retirement.member.aggregate_version,
            fenced_node_id: Some(provider_node_id),
            completed_at: removal_completed_at,
            correlation_id: Uuid::now_v7(),
        })
        .await
        .expect("complete existing Workloads retirement");
    assert_eq!(retired.value.lifecycle, WorkloadReplicaLifecycle::Retired);
    assert_eq!(workloads.outbox_events().await.len(), 5);

    let start_command = StartDurableCellApplication {
        organization_id,
        project_id,
        environment_id,
        application_id: record.application.id,
        expected_version: stopped_application.aggregate_version,
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "restart-deployed-counters".into(),
        request_id: Uuid::now_v7(),
    };
    let start_handler =
        StartDurableCellApplicationHandler::new(applications.clone(), workload_port);
    let restarted = start_handler
        .execute(start_command.clone(), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("command framework")
        .expect("restart retired replica");
    assert!(!restarted.replayed);
    assert_eq!(
        restarted.record.application.desired_state,
        DurableCellApplicationDesiredState::Running
    );
    let restarted_control = workloads
        .find_workload_control(organization_id, projection.workload_id)
        .await
        .expect("restarted control");
    assert_eq!(
        restarted_control.spec.placement_policy.desired_replicas(),
        1
    );
    let restarted_replicas = workloads
        .list_workload_replicas(organization_id, projection.workload_id)
        .await
        .expect("restarted replicas");
    assert_eq!(restarted_replicas.len(), 1);
    assert_eq!(
        restarted_replicas[0].lifecycle,
        WorkloadReplicaLifecycle::Desired
    );
    assert_eq!(workloads.outbox_events().await.len(), 6);
    assert!(
        start_handler
            .execute(start_command, CqrsContext::new(ModuleRef::new()),)
            .await
            .expect("command framework")
            .expect("exact start replay")
            .replayed
    );
    assert_eq!(workloads.outbox_events().await.len(), 6);

    let denied = handler
        .execute(
            DeployDurableCellApplication {
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
                ..command
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("command framework");
    assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
}

fn state_idempotency(
    record: &DurableCellApplicationRecord,
    expected_version: u64,
    key: &str,
) -> IdempotencyRequest {
    let application = &record.application;
    let canonical = serde_json::to_vec(&CanonicalStateRequest {
        organization_id: application.organization_id,
        project_id: application.project_id,
        environment_id: application.environment_id,
        application_id: application.id,
        expected_version,
        desired_state: application.desired_state.as_str(),
    })
    .expect("canonical state request");
    IdempotencyRequest::new(
        format!(
            "organizations/{}/projects/{}/environments/{}/durable-cell-applications/{}/desired-state",
            application.organization_id,
            application.project_id,
            application.environment_id,
            application.id
        ),
        key,
        &canonical,
    )
    .expect("state idempotency")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalStateRequest<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    expected_version: u64,
    desired_state: &'a str,
}

async fn store_application_revision(
    applications: &InMemoryDurableCellApplicationRepository,
    previous: &DurableCellApplicationRecord,
    next: DurableCellApplicationRecord,
    actor_principal_id: PrincipalId,
    idempotency_key: &str,
) {
    let request_id = Uuid::now_v7();
    let event =
        DurableCellApplicationChanged::revised(&next.application, &next.revision, request_id)
            .expect("revision event");
    applications
        .revise(ReviseDurableCellApplicationWrite {
            record: next,
            expected_version: previous.application.aggregate_version,
            event,
            actor_principal_id,
            request_id,
            idempotency: IdempotencyRequest::new(
                "durable-cell-deployment-test/application-revisions",
                idempotency_key,
                request_id.as_bytes(),
            )
            .expect("revision idempotency"),
        })
        .await
        .expect("store application revision");
}

fn application_record(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actor: PrincipalId,
    profile: &DurableCellServiceProfile,
    at: chrono::DateTime<Utc>,
) -> DurableCellApplicationRecord {
    let application_id = DurableCellApplicationId::new();
    let definition =
        DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
            build_run_id: BuildRunId::new(),
            bundle_digest: digest('a'),
            bundle_size_bytes: 4096,
            main_module: "worker.mjs".into(),
            compatibility_date: "2026-08-16".into(),
            compatibility_flags: Vec::new(),
            cell_classes: vec![DurableCellClassSpec {
                name: "Counter".into(),
                state_schema: DurableCellStateSchema {
                    minimum_readable_version: 1,
                    maximum_readable_version: 1,
                    write_version: 1,
                },
            }],
            service_profile_digest: profile.digest().clone(),
            rollback_policy: DurableCellRollbackPolicy::Compatible,
        })
        .expect("application definition");
    let revision = DurableCellApplicationRevision::initial(
        organization_id,
        project_id,
        environment_id,
        application_id,
        DurableCellApplicationRevisionId::new(),
        definition,
        actor,
        at,
    )
    .expect("application revision");
    let application = DurableCellApplication::create(
        application_id,
        ResourceName::parse("Tenant counters").expect("application name"),
        &revision,
    )
    .expect("application");
    DurableCellApplicationRecord::new(application, revision).expect("application record")
}

fn service_profile() -> DurableCellServiceProfile {
    DurableCellServiceProfile::from_spec(
        crate::modules::durable_cells::domain::DurableCellServiceProfileSpec {
            public_runtime_port: "cell-public".into(),
            internal_runtime_port: "cell-internal".into(),
            health_path: "/__celld/health".into(),
            max_cell_name_bytes: 512,
            max_request_bytes: 16 * 1024 * 1024,
            max_response_bytes: 64 * 1024 * 1024,
            max_websocket_message_bytes: 1024 * 1024,
        },
    )
    .expect("Service profile")
}

fn service_template(
    profile: &DurableCellServiceProfile,
    provider_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: crate::modules::shared_kernel::domain::StorageNamespaceId,
    access_key_id: SecretVersionReference,
    secret_access_key: SecretVersionReference,
) -> ServiceTemplate {
    let publisher =
        crate::modules::durable_cells::domain::DurableCellPublisherProfile::pinned_celld_v0_2_1()
            .expect("pinned celld publisher profile");
    let artifact_digest = publisher.image_digest().clone();
    ServiceTemplate {
        artifact: OciArtifact {
            uri: publisher.image_uri().into(),
            digest: artifact_digest.to_string(),
            media_type: "application/vnd.oci.image.index.v1+json".into(),
        },
        process: compose_pinned_celld_service_process(
            provider_profile,
            storage_namespace_id,
            8080,
            8081,
            &publisher,
        )
        .expect("pinned celld Service process"),
        secrets: vec![
            SecretBinding {
                name: "s0-access-key-id".into(),
                secret_id: access_key_id.secret_id,
                version: access_key_id.version,
                target: SecretBindingTarget::Environment {
                    variable: "AWS_ACCESS_KEY_ID".into(),
                },
            },
            SecretBinding {
                name: "s0-secret-access-key".into(),
                secret_id: secret_access_key.secret_id,
                version: secret_access_key.version,
                target: SecretBindingTarget::Environment {
                    variable: "AWS_SECRET_ACCESS_KEY".into(),
                },
            },
        ],
        resources: ServiceResources {
            cpu_millis: 1000,
            memory_bytes: 512 * 1024 * 1024,
            pids: 256,
            ephemeral_storage_bytes: None,
        },
        ports: vec![
            ServicePort {
                name: profile.spec().public_runtime_port.clone(),
                container_port: 8080,
            },
            ServicePort {
                name: profile.spec().internal_runtime_port.clone(),
                container_port: 8081,
            },
        ],
        health: Some(HttpHealthCheck {
            port_name: profile.spec().public_runtime_port.clone(),
            path: profile.spec().health_path.clone(),
            interval_ms: 1000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5000,
        }),
    }
}

async fn store_secret(
    repository: &InMemorySecretRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    name: &str,
    at: chrono::DateTime<Utc>,
) -> SecretVersionReference {
    let secret_id = SecretId::new();
    let (secret, version) = Secret::create(
        secret_id,
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse(name).expect("Secret name"),
        EncryptedSecretValue::new("test-key", format!("ciphertext-{secret_id}"))
            .expect("ciphertext"),
        at,
    )
    .expect("Secret");
    repository
        .create(CreateSecretWrite {
            event: SecretChanged::created(&secret, &version, Uuid::now_v7()).expect("Secret event"),
            idempotency: IdempotencyRequest::new(
                "durable-cell-deployment-test/secrets",
                secret_id.to_string(),
                secret_id.as_uuid().as_bytes(),
            )
            .expect("Secret idempotency"),
            secret,
            version,
        })
        .await
        .expect("store Secret");
    SecretVersionReference::new(secret_id, 1).expect("Secret reference")
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}
