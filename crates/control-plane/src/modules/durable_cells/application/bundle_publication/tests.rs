use super::super::provider_workload::{
    durable_cell_managed_owner_reference, project_durable_cell_provider_workload,
};
use super::*;
use crate::modules::artifacts::domain::test_support::{
    succeeded_external_build_with_output, typed_build_output,
};
use crate::modules::artifacts::InMemoryBuildRunRepository;
use crate::modules::data::{
    ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
    ObjectNamespaceFlowBinding, ObjectNamespaceKey, ObjectNamespaceProviderProfile,
    ObjectNamespaceRecoveryOperationRequest, ObjectNamespaceRecoveryPoint,
    ObjectNamespaceRecoveryPointSpec, ObjectNamespaceRetentionPolicy,
    ObjectNamespaceRetentionPolicySpec, SealObjectNamespaceOperationInput,
    SealObjectNamespaceOperationOutput,
};
use crate::modules::durable_cells::domain::{
    CreateDurableCellApplicationWrite, CreateDurableCellDeploymentWrite, DurableCellApplication,
    DurableCellApplicationChanged, DurableCellApplicationDefinition,
    DurableCellApplicationDefinitionSpec, DurableCellApplicationRecord,
    DurableCellApplicationRevision, DurableCellClassSpec, DurableCellDeploymentRequest,
    DurableCellProjectionIdentity, DurableCellProviderBinding, DurableCellRollbackPolicy,
    DurableCellServiceProfile, DurableCellServiceProfileSpec, DurableCellStateSchema,
    DurableCellStorageBinding,
};
use crate::modules::durable_cells::infrastructure::{
    ArtifactsDurableCellBuildArtifactAdapter, ExecutionsDurableCellExecutionAdapter,
    InMemoryDurableCellApplicationRepository, InMemoryDurableCellDeploymentRepository,
    WorkloadsDurableCellWorkloadAdapter,
};
use crate::modules::durable_cells::{
    DurableCellBuildArtifactRequest, IDurableCellExecutionPort, IDurableCellWorkloadPort,
};
use crate::modules::executions::domain::{ExecutionOutcome, ExecutionStatus, IExecutionRepository};
use crate::modules::executions::InMemoryExecutionRepository;
use crate::modules::operations::domain::entities::{
    OperationProjection, OperationRequest, OperationStatus,
};
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::operations::{IOperationRepository, InMemoryOperationRepository};
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::events::EnvironmentCreated;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, DurableCellApplicationId, DurableCellApplicationRevisionId,
    EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OperationId, OrganizationId,
    PrincipalId, ProjectId, ResourceName, SecretId, SecretVersionReference, SourceRevisionId,
};
use crate::modules::workloads::application::project_runtime_secrets;
use crate::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentRequested, HttpHealthCheck,
    IWorkloadReplicaDeploymentRepository, IWorkloadReplicaRetirementRepository,
    IWorkloadRepository, IWorkloadWriterFenceRepository, InMemoryWorkloadRepository, OciArtifact,
    ReconfigureReplicaSetWrite, ReplicaRetirementCompletion, ReplicaRetirementDispatch,
    ReplicaRuntimeFence, SecretBinding, SecretBindingTarget, ServicePort, ServiceResources,
    ServiceTemplate, Workload, WorkloadControlSpec, WorkloadRevision, WorkloadWriterFenceReceipt,
    WorkloadWriterFenceReceiptSpec,
};
use chrono::{Duration, Utc};

fn execution_port(
    projects: Arc<InMemoryProjectsRepository>,
    executions: Arc<InMemoryExecutionRepository>,
) -> Arc<dyn IDurableCellExecutionPort> {
    Arc::new(ExecutionsDurableCellExecutionAdapter::new(
        projects, executions,
    ))
}

fn workload_port(
    applications: Arc<InMemoryDurableCellApplicationRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
) -> Arc<dyn IDurableCellWorkloadPort> {
    workload_port_with_writer_fences(applications, workloads.clone(), workloads)
}

fn workload_port_with_writer_fences(
    applications: Arc<InMemoryDurableCellApplicationRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    writer_fences: Arc<dyn IWorkloadWriterFenceRepository>,
) -> Arc<dyn IDurableCellWorkloadPort> {
    Arc::new(WorkloadsDurableCellWorkloadAdapter::new(
        applications.clone(),
        workloads,
        writer_fences,
    ))
}

struct StaticWriterFenceRepository {
    receipt: WorkloadWriterFenceReceipt,
}

#[async_trait]
impl IWorkloadWriterFenceRepository for StaticWriterFenceRepository {
    async fn latest_writer_fence(
        &self,
        organization_id: OrganizationId,
        workload_id: crate::modules::shared_kernel::domain::WorkloadId,
    ) -> Result<Option<WorkloadWriterFenceReceipt>, RepositoryError> {
        let spec = self.receipt.spec();
        if spec.organization_id != organization_id || spec.workload_id != workload_id {
            return Ok(None);
        }
        Ok(Some(self.receipt.clone()))
    }
}

#[tokio::test]
async fn gate_creates_one_exact_replay_safe_node_bound_publication_execution(
) -> Result<(), Box<dyn std::error::Error>> {
    let at = Utc::now() - Duration::seconds(2);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let node_id = NodeId::new();

    let projects = Arc::new(InMemoryProjectsRepository::new());
    let environment = Environment::create(
        organization_id,
        project_id,
        environment_id,
        EnvironmentName::parse("Publication fixture")?,
        at,
    );
    IEnvironmentRepository::create(
        projects.as_ref(),
        environment.clone(),
        EnvironmentCreated::envelope(&environment, Uuid::now_v7())?,
        IdempotencyRequest::new(
            "durable-cell-publication-test/environment",
            "create",
            environment_id.as_uuid().as_bytes(),
        )?,
    )
    .await?;

    let bundle_digest = digest('b')?;
    let bundle_output =
        typed_build_output(bundle_digest.as_str(), DURABLE_CELL_BUNDLE_MEDIA_TYPE, 4096);
    let build = succeeded_external_build_with_output(
        organization_id,
        project_id,
        environment_id,
        SourceRevisionId::new(),
        bundle_output,
        at,
    );
    let build_run_id = build.id;
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    builds.seed_build(build).await;
    let build_artifacts: Arc<dyn IDurableCellBuildArtifactPort> = Arc::new(
        ArtifactsDurableCellBuildArtifactAdapter::new(builds.clone()),
    );
    let bundle = build_artifacts
        .find_published_bundle(&DurableCellBuildArtifactRequest {
            organization_id,
            project_id,
            environment_id,
            build_run_id,
        })
        .await?;

    let service_profile = DurableCellServiceProfile::from_spec(DurableCellServiceProfileSpec {
        public_runtime_port: "cell-public".into(),
        internal_runtime_port: "cell-internal".into(),
        health_path: "/__celld/health".into(),
        max_cell_name_bytes: 512,
        max_request_bytes: 16 * 1024 * 1024,
        max_response_bytes: 64 * 1024 * 1024,
        max_websocket_message_bytes: 1024 * 1024,
    })?;
    let application_id = DurableCellApplicationId::new();
    let definition = definition(
        build_run_id,
        bundle_digest,
        service_profile.digest().clone(),
    )?;
    let application_revision = DurableCellApplicationRevision::initial(
        organization_id,
        project_id,
        environment_id,
        application_id,
        DurableCellApplicationRevisionId::new(),
        definition,
        actor,
        at,
    )?;
    let application = DurableCellApplication::create(
        application_id,
        ResourceName::parse("Publication fixture")?,
        &application_revision,
    )?;
    let application_record =
        DurableCellApplicationRecord::new(application.clone(), application_revision.clone())?;
    let applications = Arc::new(InMemoryDurableCellApplicationRepository::new());
    let application_request_id = Uuid::now_v7();
    applications
        .create(CreateDurableCellApplicationWrite {
            record: application_record,
            event: DurableCellApplicationChanged::created(
                &application,
                &application_revision,
                application_request_id,
            )?,
            actor_principal_id: actor,
            request_id: application_request_id,
            idempotency: IdempotencyRequest::new(
                "durable-cell-publication-test/application",
                "create",
                application_id.as_uuid().as_bytes(),
            )?,
        })
        .await?;

    let projection =
        DurableCellProjectionIdentity::for_current_revision(&application, &application_revision)?;
    let storage_profile = ObjectNamespaceProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/s0.1/object-namespace-provider-profile.acl"
    )))?;
    let access_key = SecretVersionReference::new(SecretId::new(), 1)?;
    let secret_access_key = SecretVersionReference::new(SecretId::new(), 2)?;
    let credentials =
        ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
            organization_id,
            project_id,
            environment_id,
            namespace_id: projection.storage_namespace_id,
            generation: 1,
            provider_profile_digest: storage_profile.digest().clone(),
            access_key_id: access_key,
            secret_access_key,
            session_token: None,
        })?;
    let retention =
        ObjectNamespaceRetentionPolicy::from_spec(ObjectNamespaceRetentionPolicySpec {
            minimum_sealed_recovery_points: 2,
            maximum_sealed_recovery_points: 24,
            maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
            deletion_grace_period_seconds: 24 * 60 * 60,
        })?;
    let storage = DurableCellStorageBinding::for_current_revision(
        &application,
        &application_revision,
        &projection,
        &credentials,
        &retention,
    )?;
    let publisher = DurableCellPublisherProfile::pinned_celld_v0_2_1()?;
    let service_template = service_template(
        &publisher,
        &service_profile,
        &storage_profile,
        projection.storage_namespace_id,
        access_key,
        secret_access_key,
    );
    let workload_revision = WorkloadRevision::create(
        projection.workload_revision_id,
        projection.workload_id,
        1,
        service_template,
        at + Duration::milliseconds(1),
    )?;
    let provider_workload = project_durable_cell_provider_workload(&workload_revision)?;
    let provider = DurableCellProviderBinding::for_current_revision(
        &application,
        &application_revision,
        &projection,
        &service_profile,
        &provider_workload,
    )?;
    let control = WorkloadControlSpec::managed_replica_set_in_pool(
        durable_cell_managed_owner_reference(&projection)?,
        1,
        1,
        None,
    )?;
    let correlation = DurableCellDeployment::bind(
        projection.clone(),
        storage,
        Some(&storage_profile),
        provider,
        Sha256Digest::parse(control.placement_policy.digest())?,
        DurableCellDeploymentRequest {
            requested_by: actor,
            request_id: Uuid::now_v7(),
            requested_at: at + Duration::milliseconds(2),
        },
    )?;
    let deployments = Arc::new(InMemoryDurableCellDeploymentRepository::new());
    deployments
        .create(CreateDurableCellDeploymentWrite {
            deployment: correlation.clone(),
            idempotency: IdempotencyRequest::new(
                "durable-cell-publication-test/correlation",
                "create",
                projection.application_revision_id.as_uuid().as_bytes(),
            )?,
        })
        .await?;

    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let workload = Workload::create(
        projection.workload_id,
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("Durable Cell provider")?,
        at,
    );
    let deployment = Deployment::create(
        projection.deployment_id,
        organization_id,
        projection.workload_id,
        projection.workload_revision_id,
        projection.operation_id,
        correlation.requested_at,
    );
    let operation = OperationRequest::new(
        projection.operation_id,
        organization_id,
        OperationSubject::new("deployment", projection.deployment_id.as_uuid())?,
        WorkflowIdentity::new("cloud.deployment", "4")?,
        serde_json::json!({
            "deploymentId": projection.deployment_id,
            "organizationId": organization_id,
            "revisionId": projection.workload_revision_id,
            "workloadId": projection.workload_id,
        }),
        correlation.requested_at,
    );
    let deployment_event =
        DeploymentRequested::envelope(&deployment, &workload_revision, Uuid::now_v7())?;
    workloads
        .create_deployment(CreateDeploymentBundle {
            workload,
            control,
            revision: workload_revision.clone(),
            deployment,
            operation,
            idempotency: IdempotencyRequest::new(
                "durable-cell-publication-test/workload",
                "create",
                projection.workload_revision_id.as_uuid().as_bytes(),
            )?,
            event: deployment_event,
        })
        .await?;
    let deployment = workloads
        .find_deployment(organization_id, projection.deployment_id)
        .await?;
    let deployment = workloads
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            correlation.requested_at + Duration::milliseconds(1),
        )
        .await?;
    workloads
        .assign_node(
            deployment.id,
            deployment.aggregate_version,
            node_id,
            correlation.requested_at + Duration::milliseconds(2),
        )
        .await?;

    let executions = Arc::new(InMemoryExecutionRepository::new());
    let operations = Arc::new(InMemoryOperationRepository::new());
    let gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        deployments.clone(),
        build_artifacts.clone(),
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(
            workload_port(applications.clone(), workloads.clone()),
            operations.clone(),
        ),
        execution_port(projects.clone(), executions.clone()),
    );
    let request = WorkloadPrestartGateRequest {
        organization_id,
        deployment_id: projection.deployment_id,
        operation_id: projection.operation_id,
        workload_id: projection.workload_id,
        workload_revision_id: projection.workload_revision_id,
        node_id,
        cancellation_requested: false,
        deadline_at: Utc::now() + Duration::minutes(10),
        now: Utc::now(),
    };

    let first = gate.reconcile(&request).await?;
    assert!(matches!(first, WorkloadPrestartGateStatus::Pending { .. }));
    let execution_id = publication_execution_id(projection.workload_revision_id);
    let execution = executions
        .find(organization_id, execution_id)
        .await?
        .ok_or("publication Execution")?;
    assert_eq!(execution.status, ExecutionStatus::Queued);
    assert_eq!(execution.target_node_id, Some(node_id));
    assert_eq!(execution.template.artifact.uri, publisher.image_uri());
    assert_eq!(
        execution.template.process.args,
        vec![
            "deploy".to_owned(),
            publisher.bundle_mount().to_owned(),
            "--bucket".to_owned(),
            format!(
                "s3://{}/{}/{}",
                storage_profile.spec().bucket,
                storage_profile.spec().prefix,
                projection.storage_namespace_id
            ),
            "--endpoint".to_owned(),
            storage_profile.spec().endpoint.clone(),
            "--region".to_owned(),
            storage_profile.spec().region.clone(),
        ]
    );
    let policy = execution.task_policy.as_ref().ok_or("publication policy")?;
    assert_eq!(
        serde_json::to_value(policy.secrets())?,
        serde_json::to_value(project_runtime_secrets(&workload_revision)?)?
    );
    assert_eq!(policy.mounts().len(), 1);
    let mount = &policy.mounts()[0];
    assert_eq!(mount.target(), publisher.bundle_mount());
    assert_eq!(mount.artifact_uri()?, bundle.uri);
    assert_eq!(mount.artifact_digest().as_str(), bundle.digest);
    assert_eq!(mount.artifact_media_type(), DURABLE_CELL_BUNDLE_MEDIA_TYPE);
    assert_eq!(policy.semantics_profile_digest(), publisher.digest());
    assert_eq!(
        policy.authority().digest(),
        &publication_authority_digest(&correlation, node_id, &bundle, &publisher)?
    );

    let replay = gate.reconcile(&request).await?;
    assert_eq!(replay, first);
    assert_eq!(executions.outbox_events().await.len(), 1);
    assert_eq!(
        executions
            .find(organization_id, execution_id)
            .await?
            .ok_or("replayed publication Execution")?,
        execution
    );

    let mut completed_execution = execution.clone();
    let expected_version = completed_execution.aggregate_version;
    completed_execution.schedule(
        node_id,
        digest('1')?.to_string(),
        request.now + Duration::milliseconds(1),
    )?;
    completed_execution = executions
        .save(completed_execution, expected_version)
        .await?;
    let expected_version = completed_execution.aggregate_version;
    completed_execution.dispatch(
        NodeCommandId::new(),
        request.now + Duration::milliseconds(2),
    )?;
    completed_execution = executions
        .save(completed_execution, expected_version)
        .await?;
    let expected_version = completed_execution.aggregate_version;
    completed_execution.begin_cleanup(
        ExecutionOutcome::Succeeded { exit_code: 0 },
        request.now + Duration::milliseconds(3),
    )?;
    completed_execution = executions
        .save(completed_execution, expected_version)
        .await?;
    let expected_version = completed_execution.aggregate_version;
    completed_execution.complete_cleanup(request.now + Duration::milliseconds(4))?;
    completed_execution = executions
        .save(completed_execution, expected_version)
        .await?;
    assert_eq!(completed_execution.status, ExecutionStatus::Succeeded);

    let queued_executions = Arc::new(InMemoryExecutionRepository::new());
    let queued_gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        deployments.clone(),
        build_artifacts.clone(),
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(
            workload_port(applications.clone(), workloads.clone()),
            operations.clone(),
        ),
        execution_port(projects.clone(), queued_executions.clone()),
    );
    assert!(matches!(
        queued_gate.reconcile(&request).await?,
        WorkloadPrestartGateStatus::Pending { .. }
    ));
    assert_eq!(queued_executions.outbox_events().await.len(), 1);

    let cancelling_executions = Arc::new(InMemoryExecutionRepository::new());
    let cancelling_gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        deployments.clone(),
        build_artifacts.clone(),
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(
            workload_port(applications.clone(), workloads.clone()),
            operations.clone(),
        ),
        execution_port(projects.clone(), cancelling_executions.clone()),
    );
    assert!(matches!(
        cancelling_gate.reconcile(&request).await?,
        WorkloadPrestartGateStatus::Pending { .. }
    ));
    let cancellation_request = WorkloadPrestartGateRequest {
        cancellation_requested: true,
        now: request.now + Duration::milliseconds(5),
        ..request.clone()
    };
    assert!(matches!(
        cancelling_gate.reconcile(&cancellation_request).await?,
        WorkloadPrestartGateStatus::Pending { .. }
    ));
    let cancelling_execution = cancelling_executions
        .find(organization_id, execution_id)
        .await?
        .ok_or("cancelling publication Execution")?;
    assert_eq!(cancelling_execution.status, ExecutionStatus::Cancelling);
    assert_eq!(
        cancelling_execution.cancellation_requested_at,
        Some(canonical_timestamp(cancellation_request.now))
    );
    let cancellation_version = cancelling_execution.aggregate_version;
    assert!(matches!(
        cancelling_gate.reconcile(&cancellation_request).await?,
        WorkloadPrestartGateStatus::Pending { .. }
    ));
    assert_eq!(
        cancelling_executions
            .find(organization_id, execution_id)
            .await?
            .ok_or("replayed cancelling publication Execution")?
            .aggregate_version,
        cancellation_version
    );

    let binding = workloads
        .find_deployment_replica_binding(organization_id, projection.deployment_id)
        .await?;
    let owner = durable_cell_managed_owner_reference(&projection)?;
    let retirement_requested_at = canonical_timestamp(request.now + Duration::milliseconds(10));
    let control = workloads
        .find_workload_control(organization_id, projection.workload_id)
        .await?;
    let stopped = workloads
        .reconfigure_replica_set(ReconfigureReplicaSetWrite {
            organization_id,
            workload_id: projection.workload_id,
            expected_control_version: control.aggregate_version,
            expected_policy_generation: control.spec.placement_policy.generation(),
            desired_replicas: 0,
            managed_owner: Some(owner.clone()),
            idempotency: IdempotencyRequest::new(
                "durable-cell-publication-test/replicas",
                "stop",
                b"stop",
            )?,
            correlation_id: Uuid::now_v7(),
            requested_at: retirement_requested_at,
        })
        .await?;
    let retirement = workloads
        .pending_replica_retirements(10)
        .await?
        .into_iter()
        .next()
        .ok_or("retiring publication fixture")?;
    let retirement_command_id = NodeCommandId::new();
    let dispatched = workloads
        .dispatch_replica_retirement(ReplicaRetirementDispatch {
            organization_id,
            workload_id: projection.workload_id,
            replica_id: retirement.replica.id,
            replica_generation: retirement.replica.generation,
            expected_replica_version: retirement.replica.aggregate_version,
            command_id: retirement_command_id,
            dispatched_at: retirement_requested_at + Duration::milliseconds(1),
        })
        .await?;
    let fenced_at = canonical_timestamp(retirement_requested_at + Duration::milliseconds(2));
    let fenced = workloads
        .record_replica_runtime_fenced(
            ReplicaRuntimeFence {
                organization_id,
                workload_id: projection.workload_id,
                replica_id: retirement.replica.id,
                replica_generation: retirement.replica.generation,
                expected_replica_version: dispatched.aggregate_version,
                command_id: retirement_command_id,
                fenced_at,
            },
            None,
        )
        .await?;
    workloads
        .complete_replica_retirement(ReplicaRetirementCompletion {
            organization_id,
            workload_id: projection.workload_id,
            replica_id: retirement.replica.id,
            replica_generation: retirement.replica.generation,
            expected_replica_version: fenced.aggregate_version,
            member_id: retirement.member.id,
            expected_member_version: retirement.member.aggregate_version,
            fenced_node_id: Some(node_id),
            completed_at: retirement_requested_at + Duration::milliseconds(3),
            correlation_id: Uuid::now_v7(),
        })
        .await?;
    let restarted = workloads
        .reconfigure_replica_set(ReconfigureReplicaSetWrite {
            organization_id,
            workload_id: projection.workload_id,
            expected_control_version: stopped.control.aggregate_version,
            expected_policy_generation: stopped.control.spec.placement_policy.generation(),
            desired_replicas: 1,
            managed_owner: Some(owner.clone()),
            idempotency: IdempotencyRequest::new(
                "durable-cell-publication-test/replicas",
                "start",
                b"start",
            )?,
            correlation_id: Uuid::now_v7(),
            requested_at: retirement_requested_at + Duration::milliseconds(4),
        })
        .await?;
    assert_eq!(restarted.replicas[0].generation, 2);
    let candidate = workloads
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .next()
        .ok_or("restarted publication fixture")?;
    let materialization = workloads
        .materialize_replica_deployment(
            candidate,
            retirement_requested_at + Duration::milliseconds(5),
        )
        .await?
        .ok_or("restarted deployment materialization")?;
    let restarted_deployment = workloads
        .mark_resolving(
            materialization.deployment.id,
            materialization.deployment.aggregate_version,
            retirement_requested_at + Duration::milliseconds(6),
        )
        .await?;
    let restarted_node_id = NodeId::new();
    let restarted_deployment = workloads
        .assign_node(
            restarted_deployment.id,
            restarted_deployment.aggregate_version,
            restarted_node_id,
            retirement_requested_at + Duration::milliseconds(7),
        )
        .await?;
    let restart_request = WorkloadPrestartGateRequest {
        deployment_id: restarted_deployment.id,
        operation_id: restarted_deployment.operation_id,
        node_id: restarted_node_id,
        now: retirement_requested_at + Duration::milliseconds(8),
        deadline_at: retirement_requested_at + Duration::minutes(10),
        ..request.clone()
    };
    let restarted_binding = workloads
        .find_deployment_replica_binding(organization_id, restarted_deployment.id)
        .await?;
    assert_eq!(restarted_binding.replica_generation, 2);
    let prior_operation_id = OperationId::new();
    let receipt = WorkloadWriterFenceReceipt::issue(WorkloadWriterFenceReceiptSpec {
        organization_id,
        project_id,
        environment_id,
        workload_id: projection.workload_id,
        workload_revision_id: projection.workload_revision_id,
        workload_revision_generation: retirement.revision.generation,
        replica_id: binding.replica_id,
        replica_ordinal: 0,
        writer_epoch: retirement.replica.generation,
        member_id: binding.member_id,
        placement_generation: binding.placement_generation,
        managed_owner: owner,
        node_id,
        runtime_unit_id: binding.runtime_unit_id.clone(),
        command_id: retirement_command_id,
        command_payload_digest: digest('c')?,
        acknowledgement_digest: digest('d')?,
        continuation_operation_id: prior_operation_id,
        fenced_at,
    })?;
    let prior_operation =
        ObjectNamespaceRecoveryOperationRequest::seal(SealObjectNamespaceOperationInput {
            operation_id: prior_operation_id,
            organization_id,
            source: ObjectNamespaceFlowBinding {
                provider_profile: storage_profile.clone(),
                credentials: credentials.clone(),
            },
            previous_recovery_point: None,
            writer_epoch: retirement.replica.generation,
            writer_fence_receipt_digest: receipt.digest().clone(),
            sealed_at: fenced_at,
        })?;
    operations.enqueue(prior_operation.clone()).await?;
    let failed_operations = Arc::new(InMemoryOperationRepository::new());
    failed_operations.enqueue(prior_operation).await?;
    let sealed_gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        deployments.clone(),
        build_artifacts.clone(),
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(
            workload_port_with_writer_fences(
                applications.clone(),
                workloads.clone(),
                Arc::new(StaticWriterFenceRepository {
                    receipt: receipt.clone(),
                }),
            ),
            operations.clone(),
        ),
        execution_port(projects.clone(), executions.clone()),
    );

    assert!(matches!(
        sealed_gate.reconcile(&request).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(matches!(
        sealed_gate.reconcile(&restart_request).await?,
        WorkloadPrestartGateStatus::Pending { .. }
    ));
    assert_eq!(executions.outbox_events().await.len(), 1);

    let scope_drift_credentials =
        ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
            organization_id,
            project_id: ProjectId::new(),
            environment_id,
            namespace_id: projection.storage_namespace_id,
            generation: 1,
            provider_profile_digest: storage_profile.digest().clone(),
            access_key_id: access_key,
            secret_access_key,
            session_token: None,
        })?;
    let scope_drift_operations = Arc::new(InMemoryOperationRepository::new());
    scope_drift_operations
        .enqueue(ObjectNamespaceRecoveryOperationRequest::seal(
            SealObjectNamespaceOperationInput {
                operation_id: prior_operation_id,
                organization_id,
                source: ObjectNamespaceFlowBinding {
                    provider_profile: storage_profile.clone(),
                    credentials: scope_drift_credentials,
                },
                previous_recovery_point: None,
                writer_epoch: retirement.replica.generation,
                writer_fence_receipt_digest: receipt.digest().clone(),
                sealed_at: fenced_at,
            },
        )?)
        .await?;
    let scope_drift_gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        deployments.clone(),
        build_artifacts.clone(),
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(
            workload_port_with_writer_fences(
                applications.clone(),
                workloads.clone(),
                Arc::new(StaticWriterFenceRepository {
                    receipt: receipt.clone(),
                }),
            ),
            scope_drift_operations,
        ),
        execution_port(projects.clone(), executions.clone()),
    );
    assert!(matches!(
        scope_drift_gate.reconcile(&restart_request).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(executions.outbox_events().await.len(), 1);

    failed_operations
        .upsert_projection(OperationProjection {
            operation_id: prior_operation_id,
            status: OperationStatus::Failed,
            last_sequence: 1,
            output: None,
            error: Some("provider seal failed".into()),
            updated_at: restart_request.now,
        })
        .await?;
    let failed_gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        deployments.clone(),
        build_artifacts.clone(),
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(
            workload_port_with_writer_fences(
                applications.clone(),
                workloads.clone(),
                Arc::new(StaticWriterFenceRepository {
                    receipt: receipt.clone(),
                }),
            ),
            failed_operations,
        ),
        execution_port(projects.clone(), executions.clone()),
    );
    assert!(matches!(
        failed_gate.reconcile(&restart_request).await?,
        WorkloadPrestartGateStatus::Failed { .. }
    ));
    assert_eq!(executions.outbox_events().await.len(), 1);

    let recovery_point = ObjectNamespaceRecoveryPoint::seal(ObjectNamespaceRecoveryPointSpec {
        namespace_id: projection.storage_namespace_id,
        sequence: 1,
        writer_epoch: retirement.replica.generation,
        provider_profile_digest: storage_profile.digest().clone(),
        manifest_key: ObjectNamespaceKey::parse("manifests/prior-writer.json")?,
        manifest_digest: digest('e')?,
        state_digest: digest('f')?,
        state_size_bytes: 1,
        predecessor_digest: None,
        sealed_at: fenced_at + Duration::milliseconds(1),
    })?;
    operations
        .upsert_projection(OperationProjection {
            operation_id: prior_operation_id,
            status: OperationStatus::Succeeded,
            last_sequence: 1,
            output: Some(serde_json::to_value(SealObjectNamespaceOperationOutput {
                recovery_point,
            })?),
            error: None,
            updated_at: restart_request.now,
        })
        .await?;
    let queued_restart_gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        deployments.clone(),
        build_artifacts.clone(),
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(
            workload_port_with_writer_fences(
                applications.clone(),
                workloads.clone(),
                Arc::new(StaticWriterFenceRepository {
                    receipt: receipt.clone(),
                }),
            ),
            operations.clone(),
        ),
        execution_port(projects.clone(), queued_executions.clone()),
    );
    assert!(matches!(
        queued_restart_gate.reconcile(&restart_request).await?,
        WorkloadPrestartGateStatus::Failed { .. }
    ));
    assert_eq!(queued_executions.outbox_events().await.len(), 1);

    assert!(matches!(
        sealed_gate.reconcile(&restart_request).await?,
        WorkloadPrestartGateStatus::Ready { .. }
    ));
    assert_eq!(executions.outbox_events().await.len(), 1);
    assert_eq!(
        executions
            .find(organization_id, execution_id)
            .await?
            .ok_or("adopted publication Execution")?,
        completed_execution
    );

    let mut legacy_correlation = correlation;
    legacy_correlation.storage_provider_profile_acl = None;
    legacy_correlation.validate()?;
    let legacy_deployments = Arc::new(InMemoryDurableCellDeploymentRepository::new());
    legacy_deployments
        .create(CreateDurableCellDeploymentWrite {
            deployment: legacy_correlation,
            idempotency: IdempotencyRequest::new(
                "durable-cell-publication-test/legacy-correlation",
                "create",
                projection.application_revision_id.as_uuid().as_bytes(),
            )?,
        })
        .await?;
    let legacy_executions = Arc::new(InMemoryExecutionRepository::new());
    let legacy_gate = DurableCellBundlePublicationGate::new(
        applications.clone(),
        legacy_deployments,
        build_artifacts,
        workload_port(applications.clone(), workloads.clone()),
        DurableCellPriorWriterSeal::new(workload_port(applications, workloads), operations),
        execution_port(projects, legacy_executions.clone()),
    );
    assert!(matches!(
        legacy_gate.reconcile(&restart_request).await?,
        WorkloadPrestartGateStatus::Ready { .. }
    ));
    assert!(legacy_executions
        .find(organization_id, execution_id)
        .await?
        .is_none());
    Ok(())
}

fn definition(
    build_run_id: BuildRunId,
    bundle_digest: Sha256Digest,
    service_profile_digest: Sha256Digest,
) -> Result<DurableCellApplicationDefinition, String> {
    DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
        build_run_id,
        bundle_digest,
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
        service_profile_digest,
        rollback_policy: DurableCellRollbackPolicy::Compatible,
    })
}

fn service_template(
    publisher: &DurableCellPublisherProfile,
    profile: &DurableCellServiceProfile,
    provider_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: StorageNamespaceId,
    access_key: SecretVersionReference,
    secret_access_key: SecretVersionReference,
) -> ServiceTemplate {
    ServiceTemplate {
        artifact: OciArtifact {
            uri: publisher.image_uri().into(),
            digest: publisher.image_digest().to_string(),
            media_type: OCI_IMAGE_INDEX_MEDIA_TYPE.into(),
        },
        process: compose_pinned_celld_service_process(
            provider_profile,
            storage_namespace_id,
            8080,
            8081,
            publisher,
        )
        .expect("pinned celld Service process"),
        secrets: vec![
            SecretBinding {
                name: "s0-access-key-id".into(),
                secret_id: access_key.secret_id,
                version: access_key.version,
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

fn digest(marker: char) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64)))
}
