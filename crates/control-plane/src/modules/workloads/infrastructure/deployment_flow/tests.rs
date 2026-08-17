use super::{DeploymentFlowConfig, DeploymentFlowDependencies, DeploymentFlowRuntime};
use crate::modules::edge::domain::events::GatewayScopeCreated;
use crate::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, IEdgeRepository, StageRoutePublication,
};
use crate::modules::edge::domain::services::IRouteTargetReader;
use crate::modules::edge::domain::{
    DomainNamePattern, GatewayCertificate, GatewayCertificateMaterial, GatewayPublication,
    GatewayRouteCutover, GatewayScope, Route, RouteHostname, RoutePath, RoutePortName, RouteState,
    RouteTarget, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::{
    EdgeDeploymentRouteUpdater, FleetGatewayCommandQueue, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig, WorkloadRouteTargetReader,
};
use crate::modules::edge::InMemoryEdgeRepository;
use crate::modules::fleet::domain::entities::{EnrollmentToken, NodeCommandDraft, NodePool};
use crate::modules::fleet::domain::events::{NodePoolChangeKind, NodePoolChanged};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodePoolRepository, INodeRepository, INodeSchedulingRepository,
    NodeEnrollmentDraft, NodeHeartbeatUpdate, NodePoolWrite,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnrollmentTokenId, EnvironmentId, GatewayCertificateId,
    GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId, NodePoolId, OperationId,
    OrganizationId, ProjectId, RepositoryError, ResourceClaimId, ResourceName, RouteId, SecretId,
    WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    AtomicResourceClaimReservation, CompiledResourceRequirements, Deployment,
    DeploymentReplicaBinding, DeploymentStatus, HttpHealthCheck, OciArtifact, OciArtifactReference,
    RequestedServiceTemplate, ResourceClaimReservation, ResourceClaimState, SecretBinding,
    SecretBindingTarget, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate, Workload,
    WorkloadControlSpec, WorkloadDesiredState, WorkloadPlacementGroup, WorkloadReplicaLifecycle,
    WorkloadRevision,
};
use crate::modules::workloads::domain::events::{DeploymentRequested, WorkloadStopRequested};
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IDeploymentFlowWorkloadRepository, IResourceClaimRepository,
    IWorkloadPlacementGroupRepository, IWorkloadReplicaDeploymentRepository, IWorkloadRepository,
    ReconfigureReplicaSetWrite, RequestWorkloadStopBundle,
};
use crate::modules::workloads::domain::services::{
    DeploymentRouteStage, DeploymentRouteUpdateRequest, IDeploymentRouteUpdater,
    IOciArtifactResolver, IWorkloadPrestartGate, OciArtifactResolutionError,
    WorkloadPrestartGateRequest, WorkloadPrestartGateStatus,
};
use crate::modules::workloads::infrastructure::{
    project_replica_runtime_spec, project_runtime_spec, InMemoryResourceClaimRepository,
    InMemoryWorkloadRepository, ReplicaDeploymentMaterializer,
};
use a3s_cloud_contracts::{
    DomainEventEnvelope, GatewayAckState, NodeCommandAck, NodeCommandFailure,
    NodeCommandLeaseRequest, NodeCommandOutcome, NodeCommandPayload, NodeGatewayAck, NodeHeartbeat,
    NodeObservationBatch, NodeResourceInventory, NodeResourceSlot, ResourceAllocation,
    ResourceKind, ResourceUnit, RuntimeObservationReport, RuntimeServiceEndpoint,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore,
    WorkflowRunStatus, WorkflowSpec,
};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeEvidence, RuntimeFeature, RuntimeHealthObservation, RuntimeHealthState,
    RuntimeObservation, RuntimeUnitClass, RuntimeUnitState, TransportProtocol,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

mod box_cancellation;
mod routed_update;
mod support;

use support::*;

struct ScriptedPrestartGate {
    normal: Mutex<WorkloadPrestartGateStatus>,
    cancellation: Mutex<WorkloadPrestartGateStatus>,
    calls: Mutex<Vec<WorkloadPrestartGateRequest>>,
}

impl ScriptedPrestartGate {
    fn new(normal: WorkloadPrestartGateStatus, cancellation: WorkloadPrestartGateStatus) -> Self {
        Self {
            normal: Mutex::new(normal),
            cancellation: Mutex::new(cancellation),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<WorkloadPrestartGateRequest> {
        self.calls.lock().expect("pre-start calls").clone()
    }

    fn set_normal(&self, status: WorkloadPrestartGateStatus) {
        *self.normal.lock().expect("normal pre-start status") = status;
    }

    fn set_cancellation(&self, status: WorkloadPrestartGateStatus) {
        *self
            .cancellation
            .lock()
            .expect("cancellation pre-start status") = status;
    }
}

#[async_trait]
impl IWorkloadPrestartGate for ScriptedPrestartGate {
    async fn reconcile(
        &self,
        request: &WorkloadPrestartGateRequest,
    ) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
        self.calls
            .lock()
            .expect("pre-start calls")
            .push(request.clone());
        let statuses = if request.cancellation_requested {
            &self.cancellation
        } else {
            &self.normal
        };
        Ok(statuses.lock().expect("pre-start status").clone())
    }
}

#[tokio::test]
async fn placement_group_deployment_flow_waits_without_partial_dispatch_and_cancels_cleanly(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("placement-group Deployment Flow")?,
        base,
    );
    let leader = template('1');
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        leader.clone(),
        base,
    )?;
    let spec = WorkloadControlSpec::unmanaged_placement_group(1, 1, 3)?;
    let policy = spec.placement_policy.clone();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let replica = workloads
        .seed_placement_group_foundation(workload.clone(), spec, revision.clone())
        .await?;
    let group = WorkloadPlacementGroup::plan(
        &workload,
        &policy,
        &revision,
        &replica,
        vec![leader, template('2'), template('3')],
        base + Duration::milliseconds(1),
    )?;
    workloads.materialize_placement_group(group).await?;
    let candidate = workloads
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .next()
        .ok_or("placement-group Deployment Flow has no candidate")?;
    let materialization = workloads
        .materialize_replica_deployment(candidate, base + Duration::milliseconds(2))
        .await?
        .ok_or("placement-group Deployment Flow materialization was skipped")?;

    let nodes = Arc::new(InMemoryNodeRepository::new());
    let engine = FlowEngine::in_memory(Arc::new(runtime(
        &workloads,
        &nodes,
        Duration::seconds(10),
    )?));
    let run_id = materialization.operation.id.to_string();
    engine
        .start_with_id(
            run_id.clone(),
            WorkflowSpec::rust_embedded(
                materialization.operation.workflow.name(),
                materialization.operation.workflow.version(),
                "a3s-cloud",
                "main",
            ),
            materialization.operation.input,
        )
        .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Suspended
    );
    let resolving = workloads
        .find_deployment(organization_id, materialization.deployment.id)
        .await?;
    assert_eq!(resolving.status, DeploymentStatus::Resolving);
    assert!(resolving.node_id.is_none() && resolving.command_id.is_none());

    let cancelling = workloads
        .mark_cancellation_requested(
            resolving.id,
            resolving.aggregate_version,
            resolving.updated_at + Duration::milliseconds(1),
        )
        .await?;
    assert_eq!(cancelling.status, DeploymentStatus::Cancelling);
    engine
        .resume_due_waits(Utc::now() + Duration::minutes(1))
        .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    let cancelled = workloads
        .find_deployment(organization_id, materialization.deployment.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert!(cancelled.node_id.is_none() && cancelled.command_id.is_none());
    let replay = workloads
        .materialize_replica_deployment(candidate, base + Duration::seconds(2))
        .await?
        .ok_or("cancelled placement-group Deployment replay was skipped")?;
    assert!(!replay.created);
    assert_eq!(replay.deployment, cancelled);
    assert_eq!(
        replay.placement_group_binding,
        materialization.placement_group_binding
    );
    Ok(())
}

#[tokio::test]
async fn placement_group_deployment_flow_marks_expired_scheduling_as_failed(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("expired placement-group scheduling")?,
        base,
    );
    let leader = template('a');
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        leader.clone(),
        base,
    )?;
    let spec = WorkloadControlSpec::unmanaged_placement_group(1, 1, 3)?;
    let policy = spec.placement_policy.clone();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let replica = workloads
        .seed_placement_group_foundation(workload.clone(), spec, revision.clone())
        .await?;
    let group = WorkloadPlacementGroup::plan(
        &workload,
        &policy,
        &revision,
        &replica,
        vec![leader, template('b'), template('c')],
        base + Duration::milliseconds(1),
    )?;
    workloads.materialize_placement_group(group).await?;
    let candidate = workloads
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .next()
        .ok_or("placement-group Deployment Flow has no candidate")?;
    let materialization = workloads
        .materialize_replica_deployment(candidate, base + Duration::milliseconds(2))
        .await?
        .ok_or("placement-group Deployment Flow materialization was skipped")?;

    let nodes = Arc::new(InMemoryNodeRepository::new());
    let engine = FlowEngine::in_memory(Arc::new(runtime(
        &workloads,
        &nodes,
        Duration::milliseconds(1),
    )?));
    let run_id = materialization.operation.id.to_string();
    engine
        .start_with_id(
            run_id.clone(),
            WorkflowSpec::rust_embedded(
                materialization.operation.workflow.name(),
                materialization.operation.workflow.version(),
                "a3s-cloud",
                "main",
            ),
            materialization.operation.input,
        )
        .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Failed
    );
    let failed = workloads
        .find_deployment(organization_id, materialization.deployment.id)
        .await?;
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert_eq!(
        failed.failure.as_deref(),
        Some("no distinct ready node set satisfies the complete placement-group plan")
    );
    assert!(failed.node_id.is_none() && failed.command_id.is_none());
    assert!(workloads
        .list_deployment_replica_member_bindings(organization_id, failed.id)
        .await?
        .iter()
        .all(|binding| binding.node_id.is_none()));
    Ok(())
}

#[tokio::test]
async fn placement_group_deployment_flow_reserves_and_places_every_member_atomically_then_cancels(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("atomic placement-group scheduling")?,
        base,
    );
    let leader = template('4');
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        leader.clone(),
        base,
    )?;
    let spec = WorkloadControlSpec::unmanaged_placement_group(1, 1, 3)?;
    let policy = spec.placement_policy.clone();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let replica = workloads
        .seed_placement_group_foundation(workload.clone(), spec, revision.clone())
        .await?;
    let group = WorkloadPlacementGroup::plan(
        &workload,
        &policy,
        &revision,
        &replica,
        vec![leader, template('5'), template('6')],
        base + Duration::milliseconds(1),
    )?;
    workloads.materialize_placement_group(group).await?;
    let candidate = workloads
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .next()
        .ok_or("placement-group Deployment Flow has no candidate")?;
    let materialization = workloads
        .materialize_replica_deployment(candidate, base + Duration::milliseconds(2))
        .await?
        .ok_or("placement-group Deployment Flow materialization was skipped")?;
    assert_eq!(materialization.operation.workflow.version(), "2");

    let nodes = Arc::new(InMemoryNodeRepository::new());
    for (name, digest) in [
        ("group-node-a", 'a'),
        ("group-node-b", 'b'),
        ("group-node-c", 'c'),
    ] {
        ready_node_with_capacity(
            &nodes,
            organization_id,
            base,
            name,
            digest,
            8_000,
            8 * 1024 * 1024 * 1024,
        )
        .await?;
    }
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let engine = FlowEngine::in_memory(Arc::new(runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(10),
    )?));
    let run_id = materialization.operation.id.to_string();
    engine
        .start_with_id(
            run_id.clone(),
            WorkflowSpec::rust_embedded(
                materialization.operation.workflow.name(),
                materialization.operation.workflow.version(),
                "a3s-cloud",
                "main",
            ),
            materialization.operation.input,
        )
        .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Suspended
    );

    let scheduled = workloads
        .find_deployment(organization_id, materialization.deployment.id)
        .await?;
    assert_eq!(scheduled.status, DeploymentStatus::Scheduled);
    assert!(scheduled.node_id.is_some() && scheduled.command_id.is_none());
    let bindings = workloads
        .list_deployment_replica_member_bindings(organization_id, scheduled.id)
        .await?;
    assert_eq!(bindings.len(), 3);
    assert_eq!(
        bindings
            .iter()
            .filter_map(|binding| binding.node_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(scheduled.node_id, bindings[0].node_id);
    for binding in &bindings {
        let claim = resource_claims
            .find(organization_id, binding.placement_group_resource_claim_id())
            .await?;
        assert_eq!(claim.state, ResourceClaimState::ReservedInDb);
        assert_eq!(claim.member_id, binding.member_id);
        assert_eq!(Some(claim.node_id), binding.node_id);
        assert_eq!(claim.placement_generation, binding.placement_generation);
    }

    workloads
        .mark_cancellation_requested(
            scheduled.id,
            scheduled.aggregate_version,
            Utc::now() + Duration::seconds(1),
        )
        .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::minutes(1))
        .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    let cancelled = workloads
        .find_deployment(organization_id, scheduled.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert_eq!(cancelled.node_id, bindings[0].node_id);
    let retained_bindings = workloads
        .list_deployment_replica_member_bindings(organization_id, scheduled.id)
        .await?;
    assert_eq!(retained_bindings, bindings);
    for binding in &retained_bindings {
        let claim = resource_claims
            .find(organization_id, binding.placement_group_resource_claim_id())
            .await?;
        assert_eq!(claim.state, ResourceClaimState::Released);
        let member = workloads
            .find_workload_replica_member(organization_id, binding.replica_id, binding.member_id)
            .await?;
        assert!(member.node_id.is_none());
        assert_eq!(member.placement_generation, binding.placement_generation);
    }
    Ok(())
}

#[tokio::test]
async fn placement_group_deployment_flow_recovers_claims_reserved_before_member_assignment(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("placement-group reservation recovery")?,
        base,
    );
    let leader = template('7');
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        leader.clone(),
        base,
    )?;
    let spec = WorkloadControlSpec::unmanaged_placement_group(1, 1, 3)?;
    let policy = spec.placement_policy.clone();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let replica = workloads
        .seed_placement_group_foundation(workload.clone(), spec, revision.clone())
        .await?;
    let group_write = WorkloadPlacementGroup::plan(
        &workload,
        &policy,
        &revision,
        &replica,
        vec![leader, template('8'), template('9')],
        base + Duration::milliseconds(1),
    )?;
    let group = workloads
        .materialize_placement_group(group_write)
        .await?
        .group;
    let candidate = workloads
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .next()
        .ok_or("placement-group Deployment Flow has no recovery candidate")?;
    let materialization = workloads
        .materialize_replica_deployment(candidate, base + Duration::milliseconds(2))
        .await?
        .ok_or("placement-group recovery Deployment was skipped")?;
    let reserved_at = Utc::now();
    let mut reservations = Vec::new();
    for (binding, plan) in materialization.member_bindings.iter().zip(&group.members) {
        let node_id = NodeId::new();
        let inventory = NodeResourceInventory::new(
            node_id.as_uuid(),
            Uuid::now_v7(),
            1,
            reserved_at,
            vec![
                NodeResourceSlot::new(
                    ResourceKind::Cpu,
                    "cpu/shared",
                    ResourceAllocation::Scalar {
                        amount: 8_000,
                        unit: ResourceUnit::MilliCpu,
                    },
                )?,
                NodeResourceSlot::new(
                    ResourceKind::Memory,
                    "memory/system",
                    ResourceAllocation::Scalar {
                        amount: 8 * 1024 * 1024 * 1024,
                        unit: ResourceUnit::Byte,
                    },
                )?,
            ],
        )?;
        let requirements =
            CompiledResourceRequirements::compile(&plan.template.resources, &inventory)?;
        reservations.push(ResourceClaimReservation {
            id: binding.placement_group_resource_claim_id(),
            binding: binding.propose_assignment(node_id, reserved_at)?,
            node_id,
            inventory,
            topology_digest: requirements.topology_digest,
            slots: requirements.slots,
            reserved_at,
        });
    }
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    resource_claims
        .reserve_atomically(AtomicResourceClaimReservation::new(reservations)?)
        .await?;

    // No node is registered in Fleet. Recovery must trust the complete durable
    // reservation instead of trying to select a different candidate set.
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let engine = FlowEngine::in_memory(Arc::new(runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims,
        Duration::seconds(10),
    )?));
    let run_id = materialization.operation.id.to_string();
    engine
        .start_with_id(
            run_id.clone(),
            WorkflowSpec::rust_embedded(
                materialization.operation.workflow.name(),
                materialization.operation.workflow.version(),
                "a3s-cloud",
                "main",
            ),
            materialization.operation.input,
        )
        .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Suspended
    );
    let scheduled = workloads
        .find_deployment(organization_id, materialization.deployment.id)
        .await?;
    assert_eq!(scheduled.status, DeploymentStatus::Scheduled);
    let bindings = workloads
        .list_deployment_replica_member_bindings(organization_id, scheduled.id)
        .await?;
    assert_eq!(bindings.len(), 3);
    assert_eq!(
        bindings
            .iter()
            .filter_map(|binding| binding.node_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    Ok(())
}

#[tokio::test]
async fn replica_set_creation_materializes_stable_ordered_identities_once(
) -> Result<(), Box<dyn std::error::Error>> {
    let requested_at = Utc::now();
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("replica-set-materialization")?,
        requested_at,
    );
    let mut bundle = deployment_bundle(
        workload.clone(),
        1,
        'a',
        requested_at,
        "replica-set-materialization",
    )?;
    bundle.control = WorkloadControlSpec::unmanaged_replica_set(1, 3)?;
    let replay = bundle.clone();
    let repository = Arc::new(InMemoryWorkloadRepository::new());

    repository.create_deployment(bundle).await?;
    assert!(repository.create_deployment(replay).await?.replayed);
    let replicas = repository
        .list_workload_replicas(organization_id, workload.id)
        .await?;
    assert_eq!(
        replicas
            .iter()
            .map(|replica| replica.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(replicas
        .iter()
        .all(|replica| replica.lifecycle == WorkloadReplicaLifecycle::Desired));
    assert_eq!(replicas[0].id.as_uuid(), workload.id.as_uuid());
    assert_ne!(replicas[1].id, replicas[2].id);
    for replica in &replicas {
        let members = repository
            .list_workload_replica_members(organization_id, replica.id)
            .await?;
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].id.as_uuid(), replica.id.as_uuid());
        assert_eq!(members[0].node_id, None);
    }
    let candidates = repository.pending_replica_deployments(10).await?;
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.replica_ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let candidate = candidates[0];
    let (left, right) = tokio::join!(
        repository.materialize_replica_deployment(candidate, requested_at),
        repository.materialize_replica_deployment(candidate, requested_at),
    );
    let left = left?.ok_or("left replica materialization was skipped")?;
    let right = right?.ok_or("right replica materialization was skipped")?;
    assert_ne!(left.created, right.created);
    assert_eq!(left.deployment, right.deployment);
    assert_eq!(left.operation, right.operation);

    let materializer = ReplicaDeploymentMaterializer::new(
        repository.clone(),
        std::time::Duration::from_millis(1),
        10,
    )?;
    let report = materializer
        .run_once(requested_at + Duration::seconds(1))
        .await?;
    assert_eq!(report.candidates, 1);
    assert_eq!(report.created, 1);
    assert!(report.failures.is_empty());
    assert_eq!(
        materializer
            .run_once(requested_at + Duration::seconds(2))
            .await?,
        crate::modules::workloads::infrastructure::ReplicaDeploymentMaterializationReport::default(
        )
    );
    let deployments = repository
        .list_deployments(organization_id, workload.id)
        .await?;
    assert_eq!(deployments.len(), 3);
    assert_eq!(
        deployments
            .iter()
            .map(|deployment| deployment.operation_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.deployment.requested")
            .count(),
        3
    );
    Ok(())
}

#[tokio::test]
async fn materialized_replica_flows_through_the_exact_replica_runtime_identity(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let first_proposed = NodeId::from_uuid(Uuid::from_u128(0x101));
    let second_proposed = NodeId::from_uuid(Uuid::from_u128(0x102));
    let third_proposed = NodeId::from_uuid(Uuid::from_u128(0x103));
    let (first_node_id, first_agent_instance_id, first_capabilities) =
        ready_node_with_capabilities(
            &nodes,
            organization_id,
            base,
            "replica-node-a",
            'a',
            1_000,
            512 * 1024 * 1024,
            first_proposed,
            capabilities(),
        )
        .await?;
    let (second_node_id, second_agent_instance_id, second_capabilities) =
        ready_node_with_capabilities(
            &nodes,
            organization_id,
            base,
            "replica-node-b",
            'b',
            1_000,
            512 * 1024 * 1024,
            second_proposed,
            capabilities(),
        )
        .await?;
    let (third_node_id, third_agent_instance_id, _) = ready_node_with_capabilities(
        &nodes,
        organization_id,
        base,
        "replica-node-c",
        'c',
        1_000,
        512 * 1024 * 1024,
        third_proposed,
        capabilities(),
    )
    .await?;
    assert_eq!(
        [first_node_id, second_node_id, third_node_id],
        [first_proposed, second_proposed, third_proposed]
    );
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let runtime =
        runtime_with_resource_claims(&workloads, &nodes, resource_claims, Duration::seconds(10))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("replica deployment Flow")?,
        base,
    );
    let mut bundle = deployment_bundle(workload.clone(), 1, '9', base, "replica-deployment-flow")?;
    bundle.control = WorkloadControlSpec::unmanaged_replica_set(1, 3)?;
    let revision = bundle.revision.clone();
    let canonical_deployment = bundle.deployment.clone();
    let canonical_operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(
            canonical_operation.id.to_string(),
            workflow_spec(),
            canonical_operation.input,
        )
        .await?;
    let canonical_lease =
        prepare_and_lease_apply(&engine, &nodes, first_node_id, first_agent_instance_id, 0).await?;
    let canonical_apply = canonical_lease
        .commands
        .iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeApply { .. }))
        .ok_or("canonical Runtime apply")?;
    record_observation(
        &nodes,
        first_node_id,
        first_agent_instance_id,
        &first_capabilities,
        canonical_apply,
        healthy_observation(
            &project_runtime_spec(&revision)?,
            RuntimeHealthState::Healthy,
        )?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        workloads
            .find_deployment(organization_id, canonical_deployment.id)
            .await?
            .status,
        DeploymentStatus::Active
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, canonical_deployment.id)
            .await?
            .node_id,
        Some(first_node_id)
    );

    let candidate = workloads
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .next()
        .ok_or("noncanonical replica candidate")?;
    assert_eq!(candidate.replica_ordinal, 1);
    let materialization = workloads
        .materialize_replica_deployment(candidate, base)
        .await?
        .ok_or("replica deployment materialization")?;
    let replica = workloads
        .find_workload_replica(
            organization_id,
            workload.id,
            materialization.candidate.replica_id,
        )
        .await?;
    let expected = project_replica_runtime_spec(&revision, &replica)?;

    let operation_id = materialization.operation.id.to_string();
    engine
        .start_with_id(
            operation_id.clone(),
            workflow_spec(),
            materialization.operation.input,
        )
        .await?;
    let leased =
        prepare_and_lease_apply(&engine, &nodes, second_node_id, second_agent_instance_id, 0)
            .await?;
    let apply = leased
        .commands
        .iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeApply { .. }))
        .ok_or("replica Runtime apply")?;
    assert_eq!(apply.aggregate_id, replica.id.as_uuid());
    let NodeCommandPayload::RuntimeApply { request, .. } = &apply.payload else {
        return Err("replica deployment emitted a non-apply command".into());
    };
    assert_eq!(request.spec, expected);
    assert_ne!(request.spec.unit_id, revision.runtime_unit_id());
    record_observation(
        &nodes,
        second_node_id,
        second_agent_instance_id,
        &second_capabilities,
        apply,
        healthy_observation(&expected, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    assert_eq!(
        engine.snapshot(&operation_id).await?.status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, materialization.deployment.id)
            .await?
            .status,
        DeploymentStatus::Active
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, materialization.deployment.id)
            .await?
            .node_id,
        Some(second_node_id)
    );

    let route_reader =
        WorkloadRouteTargetReader::new(workloads.clone(), nodes.clone(), Duration::seconds(30))?;
    let port_name = RoutePortName::parse("http")?;
    let target_set = route_reader
        .resolve_healthy_target_set(
            organization_id,
            workload.project_id,
            workload.environment_id,
            revision.id,
            &port_name,
            &[first_node_id, second_node_id],
            Utc::now(),
        )
        .await?;
    assert_eq!(
        target_set
            .for_member(first_node_id)
            .ok_or("canonical route target")?
            .target
            .runtime_unit_id,
        revision.runtime_unit_id()
    );
    assert_eq!(
        target_set
            .for_member(second_node_id)
            .ok_or("replica route target")?
            .target
            .runtime_unit_id,
        expected.unit_id
    );

    let cleanup_candidate = workloads
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .next()
        .ok_or("cleanup replica candidate")?;
    assert_eq!(cleanup_candidate.replica_ordinal, 2);
    let cleanup_materialization = workloads
        .materialize_replica_deployment(cleanup_candidate, base)
        .await?
        .ok_or("cleanup replica deployment materialization")?;
    let cleanup_replica = workloads
        .find_workload_replica(
            organization_id,
            workload.id,
            cleanup_materialization.candidate.replica_id,
        )
        .await?;
    let cleanup_spec = project_replica_runtime_spec(&revision, &cleanup_replica)?;
    engine
        .start_with_id(
            cleanup_materialization.operation.id.to_string(),
            workflow_spec(),
            cleanup_materialization.operation.input,
        )
        .await?;
    let cleanup_apply_lease =
        prepare_and_lease_apply(&engine, &nodes, third_node_id, third_agent_instance_id, 0).await?;
    let cleanup_apply = cleanup_apply_lease
        .commands
        .iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeApply { .. }))
        .ok_or("cleanup replica Runtime apply")?;
    assert_eq!(cleanup_apply.aggregate_id, cleanup_replica.id.as_uuid());
    let NodeCommandPayload::RuntimeApply { request, .. } = &cleanup_apply.payload else {
        return Err("cleanup replica deployment emitted a non-apply command".into());
    };
    assert_eq!(request.spec, cleanup_spec);
    let cleanup_apply_sequence = cleanup_apply.sequence;
    let applying = workloads
        .find_deployment(organization_id, cleanup_materialization.deployment.id)
        .await?;
    workloads
        .mark_cancellation_requested(
            applying.id,
            applying.aggregate_version,
            Utc::now().max(applying.updated_at),
        )
        .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(3))
        .await?;
    let cleanup_lease = lease(
        &nodes,
        third_node_id,
        third_agent_instance_id,
        cleanup_apply_sequence,
    )
    .await?;
    let cleanup = cleanup_lease
        .commands
        .iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }))
        .ok_or("replica Runtime cleanup")?;
    assert_eq!(cleanup.aggregate_id, cleanup_replica.id.as_uuid());

    let control = workloads
        .find_workload_control(organization_id, workload.id)
        .await?;
    workloads
        .reconfigure_replica_set(replica_set_write(
            &control,
            1,
            "route-target-scale-down",
            Utc::now() + Duration::seconds(4),
        )?)
        .await?;
    let single_target = route_reader
        .resolve_healthy_target(
            organization_id,
            workload.project_id,
            workload.environment_id,
            revision.id,
            &port_name,
            Utc::now() + Duration::seconds(4),
        )
        .await?;
    assert_eq!(single_target.node_id, first_node_id);
    assert_eq!(
        single_target.target.runtime_unit_id,
        revision.runtime_unit_id()
    );
    Ok(())
}

#[tokio::test]
async fn replica_set_reconfiguration_is_idempotent_versioned_and_concurrency_safe(
) -> Result<(), Box<dyn std::error::Error>> {
    let requested_at = Utc::now();
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("replica-set-reconfiguration")?,
        requested_at,
    );
    let bundle = deployment_bundle(
        workload.clone(),
        1,
        'a',
        requested_at,
        "replica-set-reconfiguration",
    )?;
    let repository = InMemoryWorkloadRepository::new();
    repository.create_deployment(bundle).await?;
    let initial = repository
        .find_workload_control(organization_id, workload.id)
        .await?;
    let left_write = replica_set_write(
        &initial,
        3,
        "replica-set-scale-up-left",
        requested_at + Duration::seconds(1),
    )?;
    let right_write = replica_set_write(
        &initial,
        3,
        "replica-set-scale-up-right",
        requested_at + Duration::seconds(1),
    )?;
    let (left, right) = tokio::join!(
        repository.reconfigure_replica_set(left_write.clone()),
        repository.reconfigure_replica_set(right_write.clone())
    );
    let (winner, winning_write, loser) = match (left, right) {
        (Ok(winner), Err(loser)) => (winner, left_write, loser),
        (Err(loser), Ok(winner)) => (winner, right_write, loser),
        outcomes => panic!("expected one replica-set writer, got {outcomes:?}"),
    };
    assert!(matches!(loser, RepositoryError::Conflict(_)));
    assert!(!winner.replayed);
    assert_eq!(winner.control.aggregate_version, 2);
    assert_eq!(winner.control.spec.placement_policy.generation(), 2);
    assert_eq!(
        winner
            .replicas
            .iter()
            .map(|replica| replica.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(winner
        .replicas
        .iter()
        .all(|replica| replica.lifecycle == WorkloadReplicaLifecycle::Desired));
    assert!(
        repository
            .reconfigure_replica_set(winning_write.clone())
            .await?
            .replayed
    );

    let conflicting_replay = ReconfigureReplicaSetWrite {
        desired_replicas: 2,
        idempotency: IdempotencyRequest::new(
            winning_write.idempotency.scope.clone(),
            winning_write.idempotency.key.clone(),
            b"different replica-set request",
        )?,
        ..winning_write
    };
    assert!(matches!(
        repository.reconfigure_replica_set(conflicting_replay).await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    let retiring_candidate = repository
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .find(|candidate| candidate.replica_ordinal == 1)
        .ok_or("replica candidate to fence during scale-down")?;
    let retiring_materialization = repository
        .materialize_replica_deployment(retiring_candidate, requested_at + Duration::seconds(1))
        .await?
        .ok_or("replica deployment to fence during scale-down")?;

    let scaled_down = repository
        .reconfigure_replica_set(replica_set_write(
            &winner.control,
            1,
            "replica-set-scale-down",
            requested_at + Duration::seconds(2),
        )?)
        .await?;
    assert_eq!(scaled_down.control.aggregate_version, 3);
    assert_eq!(scaled_down.control.spec.placement_policy.generation(), 3);
    assert_eq!(
        scaled_down
            .replicas
            .iter()
            .map(|replica| replica.lifecycle)
            .collect::<Vec<_>>(),
        vec![
            WorkloadReplicaLifecycle::Desired,
            WorkloadReplicaLifecycle::Retiring,
            WorkloadReplicaLifecycle::Retiring,
        ]
    );
    assert!(matches!(
        repository
            .mark_resolving(
                retiring_materialization.deployment.id,
                retiring_materialization.deployment.aggregate_version,
                requested_at + Duration::seconds(3),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.replica-set.reconfigured")
            .count(),
        2
    );
    Ok(())
}

fn replica_set_write(
    control: &crate::modules::workloads::domain::entities::WorkloadControl,
    desired_replicas: u32,
    idempotency_key: &str,
    requested_at: chrono::DateTime<Utc>,
) -> Result<ReconfigureReplicaSetWrite, Box<dyn std::error::Error>> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "organizationId": control.organization_id,
        "workloadId": control.workload_id,
        "expectedPolicyGeneration": control.spec.placement_policy.generation(),
        "desiredReplicas": desired_replicas,
    }))?;
    Ok(ReconfigureReplicaSetWrite {
        organization_id: control.organization_id,
        workload_id: control.workload_id,
        expected_control_version: control.aggregate_version,
        expected_policy_generation: control.spec.placement_policy.generation(),
        desired_replicas,
        managed_owner: control.spec.managed_owner.clone(),
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{}/workloads/{}/replica-set",
                control.organization_id, control.workload_id
            ),
            idempotency_key,
            &canonical,
        )?,
        correlation_id: Uuid::now_v7(),
        requested_at,
    })
}

fn standalone_placement_binding(
    organization_id: OrganizationId,
    node_id: NodeId,
    at: chrono::DateTime<Utc>,
) -> DeploymentReplicaBinding {
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    DeploymentReplicaBinding {
        deployment_id: DeploymentId::new(),
        organization_id,
        project_id: ProjectId::new(),
        environment_id: EnvironmentId::new(),
        workload_id,
        revision_id,
        replica_id: WorkloadReplicaId::from_uuid(workload_id.as_uuid()),
        replica_generation: 1,
        member_id: WorkloadReplicaMemberId::from_uuid(workload_id.as_uuid()),
        node_id: Some(node_id),
        placement_generation: 1,
        runtime_unit_id: format!("workload:{workload_id}:revision:{revision_id}"),
        runtime_generation: 1,
        created_at: at,
        updated_at: at,
    }
}

#[tokio::test]
async fn v4_waits_for_the_prestart_gate_before_dispatching_runtime_apply(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, _) = ready_node(&nodes, organization_id, base).await?;
    let gate = Arc::new(ScriptedPrestartGate::new(
        WorkloadPrestartGateStatus::Pending {
            reason: "bundle publication is still running".into(),
        },
        WorkloadPrestartGateStatus::CancellationReady {
            completed_at: Utc::now(),
        },
    ));
    let runtime = runtime_with_prestart_gate(
        &workloads,
        &nodes,
        Arc::new(InMemoryResourceClaimRepository::new()),
        gate.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("pre-start gate fixture")?,
            base,
        ),
        1,
        '4',
        base,
        "pre-start-gate-v4",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    let preparation_lease = lease(&nodes, node_id, agent_instance_id, 0).await?;
    let preparation = preparation_lease
        .commands
        .iter()
        .find(|command| {
            matches!(
                command.payload,
                NodeCommandPayload::ResourceClaimPrepare { .. }
            )
        })
        .ok_or("resource preparation command")?;
    acknowledge_resource_claim(&nodes, preparation).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;

    let blocked_lease = lease(&nodes, node_id, agent_instance_id, preparation.sequence).await?;
    assert!(blocked_lease.commands.is_empty());
    let calls = gate.calls();
    let first_call = calls.first().ok_or("pre-start gate call")?;
    assert_eq!(first_call.organization_id, organization_id);
    assert_eq!(first_call.deployment_id, deployment.id);
    assert_eq!(first_call.operation_id, operation.id);
    assert_eq!(first_call.workload_id, deployment.workload_id);
    assert_eq!(first_call.workload_revision_id, deployment.revision_id);
    assert_eq!(first_call.node_id, node_id);
    assert!(!first_call.cancellation_requested);

    gate.set_normal(WorkloadPrestartGateStatus::Ready {
        completed_at: Utc::now(),
    });
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let apply_lease = lease(&nodes, node_id, agent_instance_id, preparation.sequence).await?;
    let apply = apply_lease
        .commands
        .iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeApply { .. }))
        .ok_or("Runtime apply after pre-start completion")?;
    assert_eq!(apply.command_id, deployment.id.as_uuid());
    assert_eq!(
        workloads
            .find_deployment(organization_id, deployment.id)
            .await?
            .status,
        DeploymentStatus::Applying
    );
    Ok(())
}

#[tokio::test]
async fn persisted_v3_deployment_replay_does_not_adopt_the_v4_prestart_gate(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, _) = ready_node(&nodes, organization_id, base).await?;
    let gate = Arc::new(ScriptedPrestartGate::new(
        WorkloadPrestartGateStatus::Failed {
            reason: "v3 must never consult this gate".into(),
        },
        WorkloadPrestartGateStatus::Failed {
            reason: "v3 must never consult this gate".into(),
        },
    ));
    let runtime = runtime_with_prestart_gate(
        &workloads,
        &nodes,
        Arc::new(InMemoryResourceClaimRepository::new()),
        gate.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("resource claim workflow fixture")?,
            base,
        ),
        1,
        '3',
        base,
        "resource-claim-workflow-v3",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(
            operation.id.to_string(),
            resource_claim_workflow_spec(),
            operation.input,
        )
        .await?;
    let apply_lease =
        prepare_and_lease_apply(&engine, &nodes, node_id, agent_instance_id, 0).await?;
    assert!(apply_lease
        .commands
        .iter()
        .any(|command| matches!(command.payload, NodeCommandPayload::RuntimeApply { .. })));
    assert!(gate.calls().is_empty());
    assert_eq!(
        workloads
            .find_deployment(organization_id, deployment.id)
            .await?
            .status,
        DeploymentStatus::Applying
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_waits_for_prestart_cleanup_before_releasing_the_resource_claim(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, _) = ready_node(&nodes, organization_id, base).await?;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let gate = Arc::new(ScriptedPrestartGate::new(
        WorkloadPrestartGateStatus::Pending {
            reason: "bundle publication is still running".into(),
        },
        WorkloadPrestartGateStatus::Pending {
            reason: "bundle publication cancellation is still running".into(),
        },
    ));
    let runtime = runtime_with_prestart_gate(
        &workloads,
        &nodes,
        resource_claims.clone(),
        gate.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("pre-start cancellation fixture")?,
            base,
        ),
        1,
        'c',
        base,
        "pre-start-gate-cancellation",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    let preparation_lease = lease(&nodes, node_id, agent_instance_id, 0).await?;
    let preparation = preparation_lease
        .commands
        .iter()
        .find(|command| {
            matches!(
                command.payload,
                NodeCommandPayload::ResourceClaimPrepare { .. }
            )
        })
        .ok_or("resource preparation command")?;
    acknowledge_resource_claim(&nodes, preparation).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert!(
        lease(&nodes, node_id, agent_instance_id, preparation.sequence)
            .await?
            .commands
            .is_empty()
    );

    let scheduled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(scheduled.status, DeploymentStatus::Scheduled);
    workloads
        .mark_cancellation_requested(
            deployment.id,
            scheduled.aggregate_version,
            Utc::now().max(scheduled.updated_at),
        )
        .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert!(gate
        .calls()
        .iter()
        .any(|request| request.cancellation_requested));
    assert!(
        lease(&nodes, node_id, agent_instance_id, preparation.sequence)
            .await?
            .commands
            .is_empty()
    );

    gate.set_cancellation(WorkloadPrestartGateStatus::CancellationReady {
        completed_at: Utc::now(),
    });
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let release_lease = lease(&nodes, node_id, agent_instance_id, preparation.sequence).await?;
    assert!(!release_lease
        .commands
        .iter()
        .any(|command| matches!(command.payload, NodeCommandPayload::RuntimeApply { .. })));
    let release = release_lease
        .commands
        .iter()
        .find(|command| {
            matches!(
                command.payload,
                NodeCommandPayload::ResourceClaimRelease { .. }
            )
        })
        .ok_or("resource release after pre-start cancellation")?;
    acknowledge_resource_claim(&nodes, release).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;

    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, deployment.id)
            .await?
            .status,
        DeploymentStatus::Cancelled
    );
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(deployment.id.as_uuid()),
            )
            .await?
            .state,
        ResourceClaimState::Released
    );
    Ok(())
}

#[tokio::test]
async fn legacy_deployment_workflow_remains_executable_for_persisted_v1_runs(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let engine = FlowEngine::in_memory(Arc::new(runtime(
        &workloads,
        &nodes,
        Duration::seconds(10),
    )?));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("legacy deployment fixture")?,
            base,
        ),
        1,
        '0',
        base,
        "legacy-deployment-v1",
    )?;
    let revision = bundle.revision.clone();
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(
            operation.id.to_string(),
            legacy_workflow_spec(),
            operation.input,
        )
        .await?;
    let apply = lease(&nodes, node_id, agent_instance_id, 0).await?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        apply
            .commands
            .first()
            .ok_or("legacy apply command missing")?,
        healthy_observation(
            &project_runtime_spec(&revision)?,
            RuntimeHealthState::Healthy,
        )?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, deployment.id)
            .await?
            .status,
        DeploymentStatus::Active
    );
    Ok(())
}

#[tokio::test]
async fn mutable_tag_is_resolved_once_and_replay_keeps_the_persisted_digest(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, _) = ready_node(&nodes, organization_id, base).await?;
    let first_digest = format!("sha256:{}", "1".repeat(64));
    let second_digest = format!("sha256:{}", "2".repeat(64));
    let resolver = Arc::new(MovingArtifactResolver::new(first_digest.clone()));
    let workload_port: Arc<dyn IDeploymentFlowWorkloadRepository> = workloads.clone();
    let node_port: Arc<dyn INodeSchedulingRepository> = nodes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
    let runtime = DeploymentFlowRuntime::new(
        DeploymentFlowDependencies::new(
            workload_port,
            Arc::new(InMemoryResourceClaimRepository::new()),
            resolver.clone(),
            node_port,
            control_port,
            Arc::new(crate::modules::workloads::domain::services::UnroutedDeploymentRouteUpdater),
        ),
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 1, 10_000, 5_000, 1, 10_000)?,
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = requested_deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("mutable tag fixture")?,
            base,
        ),
        base,
        "mutable-tag",
    )?;
    let revision_id = bundle.revision.id;
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(
            operation.id.to_string(),
            workflow_spec(),
            operation.input.clone(),
        )
        .await?;
    let lease = prepare_and_lease_apply(&engine, &nodes, node_id, agent_instance_id, 0).await?;
    let apply = lease
        .commands
        .first()
        .ok_or("Runtime apply was not dispatched")?;
    let runtime_artifact = match &apply.payload {
        a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request, .. } => {
            &request.spec.artifact
        }
        _ => return Err("deployment dispatched a non-apply command".into()),
    };
    assert_eq!(runtime_artifact.digest, first_digest);
    assert!(runtime_artifact.uri.contains("@sha256:"));
    assert!(!runtime_artifact.uri.ends_with(":stable"));
    assert_eq!(resolver.calls(), 1);

    resolver.move_tag(second_digest);
    let history_length = engine.history(&operation.id.to_string()).await?.len();
    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    assert_eq!(
        engine.history(&operation.id.to_string()).await?.len(),
        history_length
    );
    assert_eq!(resolver.calls(), 1);
    let revision = workloads
        .find_revision(organization_id, revision_id)
        .await?;
    assert_eq!(
        revision.resolved_template()?.artifact.digest,
        runtime_artifact.digest
    );
    Ok(())
}

#[tokio::test]
async fn resolving_step_lends_only_the_bound_registry_secret_reference_to_the_resolver(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let secret_id = SecretId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    ready_node(&nodes, organization_id, base).await?;
    let resolver = Arc::new(MovingArtifactResolver::new(format!(
        "sha256:{}",
        "3".repeat(64)
    )));
    let runtime = DeploymentFlowRuntime::new(
        DeploymentFlowDependencies::new(
            workloads.clone(),
            Arc::new(InMemoryResourceClaimRepository::new()),
            resolver.clone(),
            nodes.clone(),
            nodes,
            Arc::new(crate::modules::workloads::domain::services::UnroutedDeploymentRouteUpdater),
        ),
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 1, 10_000, 5_000, 1, 10_000)?,
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = requested_deployment_bundle_with_secrets(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse("private registry fixture")?,
            base,
        ),
        base,
        "private-registry-reference",
        vec![SecretBinding {
            name: "registry".into(),
            secret_id,
            version: 7,
            target: SecretBindingTarget::RegistryCredential,
        }],
    )?;
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;

    assert_eq!(resolver.calls(), 1);
    assert_eq!(
        resolver.registry_credential(),
        Some(
            crate::modules::workloads::domain::services::OciRegistryCredentialReference {
                organization_id,
                project_id,
                environment_id,
                secret_id,
                version: 7,
            }
        )
    );
    Ok(())
}

#[tokio::test]
async fn active_workload_stop_waits_for_stopped_evidence_and_clears_active_revision(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let runtime = runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(10),
    )?;
    let store = Arc::new(FailOnceStepCompletionStore::new("stop-dispatch"));
    let engine = FlowEngine::new(store.clone(), Arc::new(runtime.clone()));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("active stop fixture")?,
            base,
        ),
        1,
        '9',
        base,
        "active-stop-deploy",
    )?;
    let revision = bundle.revision.clone();
    let deployment_operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(
            deployment_operation.id.to_string(),
            workflow_spec(),
            deployment_operation.input,
        )
        .await?;
    let apply_lease =
        prepare_and_lease_apply(&engine, &nodes, node_id, agent_instance_id, 0).await?;
    let spec = project_runtime_spec(&revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        apply_lease
            .commands
            .first()
            .ok_or("missing apply command")?,
        healthy_observation(&spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let requested_at = Utc::now();
    let mut workload = workloads
        .find_workload(organization_id, revision.workload_id)
        .await?;
    let expected_version = workload.aggregate_version;
    workload.request_stop(requested_at)?;
    let stop_operation_id = OperationId::new();
    let stop_operation = OperationRequest::new(
        stop_operation_id,
        organization_id,
        OperationSubject::new("workload", workload.id.as_uuid())?,
        WorkflowIdentity::new("cloud.workload.stop", "1")?,
        serde_json::json!({
            "operationId": stop_operation_id,
            "organizationId": organization_id,
            "requestedAt": requested_at,
            "workloadId": workload.id,
        }),
        requested_at,
    );
    let stop_request = RequestWorkloadStopBundle {
        event: WorkloadStopRequested::envelope(&workload, &stop_operation, Uuid::now_v7())?,
        idempotency: IdempotencyRequest::new("test.workload.stop", "active-stop", b"active-stop")?,
        operation: stop_operation.clone(),
        workload,
        expected_version,
    };
    let accepted = workloads
        .request_workload_stop(stop_request.clone())
        .await?;
    let replayed = workloads.request_workload_stop(stop_request).await?;
    assert!(!accepted.replayed);
    assert!(replayed.replayed);
    assert_eq!(accepted.operation.id, replayed.operation.id);

    let stop_input = stop_operation.input.clone();
    let failure = engine
        .start_with_id(
            stop_operation.id.to_string(),
            stop_workflow_spec(),
            stop_input.clone(),
        )
        .await
        .expect_err("injected crash must interrupt stop dispatch persistence");
    assert!(matches!(failure, FlowError::Store(_)));
    let stop_history = store.list(&stop_operation.id.to_string()).await?;
    assert!(stop_history.iter().any(|event| matches!(
        &event.event,
        FlowEvent::StepStarted { step_id, .. } if step_id == "stop-dispatch"
    )));
    assert!(!stop_history.iter().any(|event| matches!(
        &event.event,
        FlowEvent::StepCompleted { step_id, .. } if step_id == "stop-dispatch"
    )));
    let expected_stop_command_id = crate::modules::shared_kernel::domain::NodeCommandId::from_uuid(
        stop_operation.id.as_uuid(),
    );
    let command_before_restart = nodes
        .find_command(node_id, expected_stop_command_id)
        .await?
        .ok_or("stop command side effect was not persisted before the injected crash")?;

    drop(engine);
    let engine = FlowEngine::new(store, Arc::new(runtime));
    engine
        .start_with_id(
            stop_operation.id.to_string(),
            stop_workflow_spec(),
            stop_input,
        )
        .await?;
    assert_eq!(
        nodes
            .find_command(node_id, expected_stop_command_id)
            .await?
            .ok_or("stop command disappeared after Flow restart")?,
        command_before_restart
    );
    let stop_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        apply_lease.commands[0].sequence,
    )
    .await?;
    assert_eq!(stop_lease.commands.len(), 1);
    let stop_command = stop_lease.commands.first().ok_or("missing stop command")?;
    assert_eq!(stop_command.command_id, expected_stop_command_id.as_uuid());
    assert!(matches!(
        stop_command.payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { .. }
    ));
    assert!(workloads
        .find_workload(organization_id, revision.workload_id)
        .await?
        .active_revision_id
        .is_some());
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        stop_command,
        stopped_observation(&spec)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    release_after_runtime_fence(
        &engine,
        &nodes,
        node_id,
        agent_instance_id,
        stop_command.sequence,
    )
    .await?;
    assert_eq!(
        engine
            .snapshot(&stop_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Completed
    );
    let stopped = workloads
        .find_workload(organization_id, revision.workload_id)
        .await?;
    assert_eq!(stopped.desired_state, WorkloadDesiredState::Stopped);
    assert_eq!(stopped.active_revision_id, None);
    Ok(())
}

#[tokio::test]
async fn healthy_observation_activates_once_and_unhealthy_update_preserves_previous_revision(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let runtime = runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("health fixture")?,
        base,
    );

    let first = deployment_bundle(workload, 1, 'a', base, "healthy-first")?;
    let first_revision = first.revision.clone();
    let first_deployment = first.deployment.clone();
    let first_operation = first.operation.clone();
    workloads.create_deployment(first).await?;
    let spec = workflow_spec();
    engine
        .start_with_id(
            first_operation.id.to_string(),
            spec.clone(),
            first_operation.input.clone(),
        )
        .await?;
    assert_eq!(
        engine
            .snapshot(&first_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Suspended
    );
    let first_lease =
        prepare_and_lease_apply(&engine, &nodes, node_id, agent_instance_id, 0).await?;
    assert_eq!(first_lease.commands.len(), 1);
    assert_eq!(
        first_lease.commands[0].command_id,
        first_deployment.id.as_uuid()
    );
    let first_runtime_spec = project_runtime_spec(&first_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        &first_lease.commands[0],
        healthy_observation(&first_runtime_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine
            .snapshot(&first_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Completed
    );
    let active = workloads
        .find_deployment(organization_id, first_deployment.id)
        .await?;
    assert_eq!(active.status, DeploymentStatus::Active);
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(first_deployment.id.as_uuid()),
            )
            .await?
            .state,
        crate::modules::workloads::domain::entities::ResourceClaimState::BoundToRuntimeUnit
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, first_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    let history_length = engine.history(&first_operation.id.to_string()).await?.len();
    engine
        .start_with_id(
            first_operation.id.to_string(),
            spec.clone(),
            first_operation.input,
        )
        .await?;
    assert_eq!(
        engine.history(&first_operation.id.to_string()).await?.len(),
        history_length
    );

    let selected_workload = workloads
        .find_workload(organization_id, first_deployment.workload_id)
        .await?;
    let second = deployment_bundle(selected_workload, 2, 'b', Utc::now(), "unhealthy-update")?;
    let second_revision = second.revision.clone();
    let second_deployment = second.deployment.clone();
    let second_operation = second.operation.clone();
    workloads.create_deployment(second).await?;
    engine
        .start_with_id(
            second_operation.id.to_string(),
            spec,
            second_operation.input.clone(),
        )
        .await?;
    let second_lease = prepare_and_lease_apply(
        &engine,
        &nodes,
        node_id,
        agent_instance_id,
        first_lease.commands[0].sequence,
    )
    .await?;
    assert_eq!(second_lease.commands.len(), 1);
    let second_runtime_spec = project_runtime_spec(&second_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        &second_lease.commands[0],
        healthy_observation(&second_runtime_spec, RuntimeHealthState::Unhealthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let failed_cleanup = lease(
        &nodes,
        node_id,
        agent_instance_id,
        second_lease.commands[0].sequence,
    )
    .await?;
    let failed_stop = failed_cleanup
        .commands
        .first()
        .ok_or("failed candidate stop was not dispatched")?;
    assert!(matches!(
        failed_stop.payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { .. }
    ));
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        failed_stop,
        stopped_observation(&second_runtime_spec)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    release_after_runtime_fence(
        &engine,
        &nodes,
        node_id,
        agent_instance_id,
        failed_stop.sequence,
    )
    .await?;
    assert_eq!(
        engine
            .snapshot(&second_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Failed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, second_deployment.id)
            .await?
            .status,
        DeploymentStatus::Failed
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, first_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(second_deployment.id.as_uuid()),
            )
            .await?
            .state,
        crate::modules::workloads::domain::entities::ResourceClaimState::Released
    );
    Ok(())
}

#[tokio::test]
async fn healthy_v4_update_retires_the_previous_runtime_before_releasing_its_claim(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let runtime = runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("healthy update retirement")?,
        base,
    );

    let first = deployment_bundle(workload, 1, '3', base, "healthy-retirement-first")?;
    let first_revision = first.revision.clone();
    let first_deployment = first.deployment.clone();
    let first_operation = first.operation.clone();
    workloads.create_deployment(first).await?;
    engine
        .start_with_id(
            first_operation.id.to_string(),
            workflow_spec(),
            first_operation.input,
        )
        .await?;
    let first_apply =
        prepare_and_lease_apply(&engine, &nodes, node_id, agent_instance_id, 0).await?;
    let first_apply = first_apply.commands.first().ok_or("first Runtime apply")?;
    let first_spec = project_runtime_spec(&first_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        first_apply,
        healthy_observation(&first_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(first_deployment.id.as_uuid()),
            )
            .await?
            .state,
        crate::modules::workloads::domain::entities::ResourceClaimState::BoundToRuntimeUnit
    );

    let active_workload = workloads
        .find_workload(organization_id, first_deployment.workload_id)
        .await?;
    let second = deployment_bundle(
        active_workload,
        2,
        '4',
        Utc::now(),
        "healthy-retirement-second",
    )?;
    let second_revision = second.revision.clone();
    let second_deployment = second.deployment.clone();
    let second_operation = second.operation.clone();
    workloads.create_deployment(second).await?;
    engine
        .start_with_id(
            second_operation.id.to_string(),
            workflow_spec(),
            second_operation.input,
        )
        .await?;
    let second_apply = prepare_and_lease_apply(
        &engine,
        &nodes,
        node_id,
        agent_instance_id,
        first_apply.sequence,
    )
    .await?;
    let second_apply = second_apply
        .commands
        .first()
        .ok_or("second Runtime apply")?;
    let second_spec = project_runtime_spec(&second_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        second_apply,
        healthy_observation(&second_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;

    let retirement_lease = lease(&nodes, node_id, agent_instance_id, second_apply.sequence).await?;
    let retirement = retirement_lease
        .commands
        .iter()
        .find(|command| {
            matches!(
                command.payload,
                a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { .. }
            )
        })
        .ok_or("previous Runtime retirement command")?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        retirement,
        stopped_observation(&first_spec)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    release_after_runtime_fence(
        &engine,
        &nodes,
        node_id,
        agent_instance_id,
        retirement.sequence,
    )
    .await?;

    assert_eq!(
        engine
            .snapshot(&second_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, second_deployment.id)
            .await?
            .status,
        DeploymentStatus::Active
    );
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(first_deployment.id.as_uuid()),
            )
            .await?
            .state,
        crate::modules::workloads::domain::entities::ResourceClaimState::Released
    );
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(second_deployment.id.as_uuid()),
            )
            .await?
            .state,
        crate::modules::workloads::domain::entities::ResourceClaimState::BoundToRuntimeUnit
    );
    Ok(())
}

#[tokio::test]
async fn durable_reservation_recovers_a_crash_before_placement_persistence(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, _, _) = ready_node(&nodes, organization_id, base).await?;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let runtime = runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("reservation crash recovery")?,
            base,
        ),
        1,
        '6',
        base,
        "reservation-before-placement",
    )?;
    let revision = bundle.revision.clone();
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    let resolving = workloads
        .mark_resolving(deployment.id, deployment.aggregate_version, Utc::now())
        .await?;
    let binding = workloads
        .find_deployment_replica_binding(organization_id, deployment.id)
        .await?;
    let inventory = nodes
        .current_resource_inventory(node_id)
        .await?
        .ok_or("ready node omitted its resource inventory")?
        .inventory;
    let requirements = CompiledResourceRequirements::compile(
        &revision.resolved_template()?.resources,
        &inventory,
    )?;
    let reserved_at = Utc::now().max(resolving.updated_at);
    let claim = resource_claims
        .reserve(ResourceClaimReservation {
            id: ResourceClaimId::from_uuid(deployment.id.as_uuid()),
            binding: binding.propose_assignment(node_id, reserved_at)?,
            node_id,
            inventory,
            topology_digest: requirements.topology_digest,
            slots: requirements.slots,
            reserved_at,
        })
        .await?
        .value;
    let before_replay = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(before_replay.status, DeploymentStatus::Resolving);
    assert_eq!(before_replay.node_id, None);

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;

    let recovered = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(recovered.node_id, Some(claim.node_id));
    assert!(matches!(
        recovered.status,
        DeploymentStatus::Scheduled
            | DeploymentStatus::Applying
            | DeploymentStatus::Verifying
            | DeploymentStatus::Active
    ));
    assert_eq!(
        resource_claims
            .find(organization_id, claim.id)
            .await?
            .node_id,
        node_id
    );
    Ok(())
}

#[tokio::test]
async fn selected_node_pool_and_maintenance_are_hard_scheduler_filters(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (left, _, _) = ready_node(&nodes, organization_id, base).await?;
    let (right, _, _) = ready_node_with_capacity(
        &nodes,
        organization_id,
        base,
        "maintenance-right",
        'e',
        8_000,
        8 * 1024 * 1024 * 1024,
    )
    .await?;
    let (third, _, _) = ready_node_with_capacity(
        &nodes,
        organization_id,
        base,
        "maintenance-third",
        'f',
        8_000,
        8 * 1024 * 1024 * 1024,
    )
    .await?;
    let mut ordered_nodes = [left, right, third];
    ordered_nodes.sort_unstable();
    let outsider = ordered_nodes[0];
    let maintained_node = ordered_nodes[1];
    let eligible_node = ordered_nodes[2];
    let mut pool = NodePool::create(
        NodePoolId::new(),
        organization_id,
        ResourceName::parse("maintenance scheduler fixture")?,
        vec![maintained_node, eligible_node],
        base + Duration::milliseconds(10),
    )?;
    let pool_id = pool.id;
    nodes
        .save(NodePoolWrite {
            expected_version: None,
            event: NodePoolChanged::envelope(
                &pool,
                NodePoolChangeKind::Created,
                pool.created_at,
                Uuid::now_v7(),
            )?,
            idempotency: IdempotencyRequest::new(
                "test/node-pools",
                "create-maintenance-scheduler",
                b"create",
            )?,
            pool: pool.clone(),
        })
        .await?;
    let previous_version = pool.aggregate_version;
    pool.schedule_maintenance(
        vec![maintained_node],
        base + Duration::milliseconds(30),
        base + Duration::hours(1),
        "kernel upgrade",
        base + Duration::milliseconds(20),
    )?;
    nodes
        .save(NodePoolWrite {
            expected_version: Some(previous_version),
            event: NodePoolChanged::envelope(
                &pool,
                NodePoolChangeKind::MaintenanceScheduled,
                pool.updated_at,
                Uuid::now_v7(),
            )?,
            idempotency: IdempotencyRequest::new(
                "test/node-pools/maintenance",
                "schedule-maintenance-scheduler",
                b"schedule",
            )?,
            pool,
        })
        .await?;

    let runtime = runtime(&workloads, &nodes, Duration::seconds(10))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let mut bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("maintenance scheduler")?,
            base,
        ),
        1,
        'd',
        base,
        "maintenance-scheduler",
    )?;
    bundle.control = WorkloadControlSpec::unmanaged_single_replica_in_pool(pool_id)?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;

    assert_eq!(
        workloads
            .find_deployment(organization_id, deployment.id)
            .await?
            .node_id,
        Some(eligible_node)
    );
    assert_ne!(eligible_node, outsider);
    Ok(())
}

#[tokio::test]
async fn capacity_exhaustion_on_the_first_node_falls_through_to_the_next_node(
) -> Result<(), Box<dyn std::error::Error>> {
    const CPU_MILLIS: u64 = 8_000;
    const MEMORY_BYTES: u64 = 8 * 1024 * 1024 * 1024;

    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (left, _, _) = ready_node_with_capacity(
        &nodes,
        organization_id,
        base,
        "capacity-left",
        '1',
        CPU_MILLIS,
        MEMORY_BYTES,
    )
    .await?;
    let (right, _, _) = ready_node_with_capacity(
        &nodes,
        organization_id,
        base,
        "capacity-right",
        '2',
        CPU_MILLIS,
        MEMORY_BYTES,
    )
    .await?;
    let (exhausted_node, available_node) = if left < right {
        (left, right)
    } else {
        (right, left)
    };
    let exhausted_inventory = nodes
        .current_resource_inventory(exhausted_node)
        .await?
        .ok_or("exhausted node omitted its resource inventory")?
        .inventory;
    let full_capacity = CompiledResourceRequirements::compile(
        &ServiceResources {
            cpu_millis: CPU_MILLIS,
            memory_bytes: MEMORY_BYTES,
            pids: 1,
            ephemeral_storage_bytes: None,
        },
        &exhausted_inventory,
    )?;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let reserved_at = Utc::now();
    resource_claims
        .reserve(ResourceClaimReservation {
            id: ResourceClaimId::new(),
            binding: standalone_placement_binding(organization_id, exhausted_node, reserved_at),
            node_id: exhausted_node,
            inventory: exhausted_inventory,
            topology_digest: full_capacity.topology_digest,
            slots: full_capacity.slots,
            reserved_at,
        })
        .await?;

    let runtime = runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("capacity fallthrough")?,
            base,
        ),
        1,
        '7',
        base,
        "capacity-fallthrough",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;

    let scheduled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(scheduled.node_id, Some(available_node));
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(deployment.id.as_uuid()),
            )
            .await?
            .node_id,
        available_node
    );
    Ok(())
}

#[tokio::test]
async fn no_eligible_node_reaches_a_persisted_failure_without_dispatch(
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let runtime = runtime(&workloads, &nodes, Duration::milliseconds(2))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("unschedulable fixture")?,
            Utc::now(),
        ),
        1,
        'c',
        Utc::now(),
        "no-node",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Failed
    );
    let failed = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert!(failed.command_id.is_none());
    assert!(failed
        .failure
        .as_deref()
        .is_some_and(|reason| reason.contains("no eligible node")));
    Ok(())
}

#[tokio::test]
async fn cancellation_before_dispatch_completes_without_creating_a_runtime_child(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let runtime = runtime(&workloads, &nodes, Duration::seconds(10))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("cancel before dispatch")?,
            base,
        ),
        1,
        'd',
        base,
        "cancel-before-dispatch",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    workloads
        .mark_cancellation_requested(deployment.id, 1, Utc::now())
        .await?;

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;

    let snapshot = engine.snapshot(&operation.id.to_string()).await?;
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.output.as_ref().and_then(|output| output
            .get("operationStatus")
            .and_then(serde_json::Value::as_str)),
        Some("cancelled")
    );
    let cancelled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert!(cancelled.command_id.is_none());
    assert!(cancelled.cleanup_command_id.is_none());
    Ok(())
}

#[tokio::test]
async fn cancellation_while_artifact_resolution_retries_completes_without_a_runtime_child(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let workload_port: Arc<dyn IDeploymentFlowWorkloadRepository> = workloads.clone();
    let node_port: Arc<dyn INodeSchedulingRepository> = nodes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes;
    let runtime = DeploymentFlowRuntime::new(
        DeploymentFlowDependencies::new(
            workload_port,
            Arc::new(InMemoryResourceClaimRepository::new()),
            Arc::new(UnusedArtifactResolver),
            node_port,
            control_port,
            Arc::new(crate::modules::workloads::domain::services::UnroutedDeploymentRouteUpdater),
        ),
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 1, 10_000, 5_000, 1, 10_000)?,
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = requested_deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("cancel resolving artifact")?,
            base,
        ),
        base,
        "cancel-resolving-artifact",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Suspended
    );
    let resolving = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(resolving.status, DeploymentStatus::Resolving);
    workloads
        .mark_cancellation_requested(
            deployment.id,
            resolving.aggregate_version,
            Utc::now().max(resolving.updated_at),
        )
        .await?;

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    engine.resume_due_retries(Utc::now()).await?;

    let snapshot = engine.snapshot(&operation.id.to_string()).await?;
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.output.as_ref().and_then(|output| output
            .get("operationStatus")
            .and_then(serde_json::Value::as_str)),
        Some("cancelled")
    );
    let cancelled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert!(cancelled.node_id.is_none());
    assert!(cancelled.command_id.is_none());
    assert!(cancelled.cleanup_command_id.is_none());
    Ok(())
}

#[tokio::test]
async fn cancellation_after_dispatch_retries_claim_release_after_durable_removal_evidence(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, _) = ready_node(&nodes, organization_id, base).await?;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let runtime = runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(10),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("cancel dispatched child")?,
            base,
        ),
        1,
        'e',
        base,
        "cancel-dispatched-child",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(
            operation.id.to_string(),
            workflow_spec(),
            operation.input.clone(),
        )
        .await?;
    let apply_lease =
        prepare_and_lease_apply(&engine, &nodes, node_id, agent_instance_id, 0).await?;
    assert_eq!(apply_lease.commands.len(), 1);
    let applying = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    workloads
        .mark_cancellation_requested(deployment.id, applying.aggregate_version, Utc::now())
        .await?;

    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let cleanup_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        apply_lease.commands[0].sequence,
    )
    .await?;
    assert_eq!(cleanup_lease.commands.len(), 1);
    assert!(matches!(
        cleanup_lease.commands[0].payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeRemove { .. }
    ));
    assert_eq!(
        workloads
            .find_deployment(organization_id, deployment.id)
            .await?
            .status,
        DeploymentStatus::CleanupPending
    );
    acknowledge_runtime_removal(&nodes, &cleanup_lease.commands[0]).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    let first_release_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        cleanup_lease.commands[0].sequence,
    )
    .await?;
    let first_release = first_release_lease
        .commands
        .iter()
        .find(|command| {
            matches!(
                command.payload,
                a3s_cloud_contracts::NodeCommandPayload::ResourceClaimRelease { .. }
            )
        })
        .ok_or("first resource release command")?;
    let a3s_cloud_contracts::NodeCommandPayload::ResourceClaimRelease {
        request: first_request,
    } = &first_release.payload
    else {
        unreachable!("selected resource release command");
    };
    let failed_at = Utc::now().max(first_release.issued_at);
    nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: first_release.command_id,
                lease_id: first_release.lease_id,
                node_id: first_release.node_id,
                sequence: first_release.sequence,
                payload_digest: first_release.payload_digest.clone(),
                completed_at: failed_at,
                outcome: NodeCommandOutcome::Failed {
                    failure: NodeCommandFailure {
                        code: "resource_claim_journal".into(),
                        message: "injected durable release failure".into(),
                        retryable: true,
                    },
                },
            },
            failed_at,
        )
        .await?;

    for offset in 3..7 {
        engine
            .resume_due_waits(Utc::now() + Duration::seconds(offset))
            .await?;
    }
    let retried_release_lease =
        lease(&nodes, node_id, agent_instance_id, first_release.sequence).await?;
    let retried_release = retried_release_lease
        .commands
        .iter()
        .find(|command| {
            matches!(
                command.payload,
                a3s_cloud_contracts::NodeCommandPayload::ResourceClaimRelease { .. }
            )
        })
        .ok_or("retried resource release command")?;
    let a3s_cloud_contracts::NodeCommandPayload::ResourceClaimRelease {
        request: retried_request,
    } = &retried_release.payload
    else {
        unreachable!("selected retried resource release command");
    };
    assert_eq!(
        retried_request.claim_generation,
        first_request.claim_generation + 1
    );
    assert_ne!(retried_request.claim_digest, first_request.claim_digest);
    assert_eq!(retried_request.binding, first_request.binding);
    acknowledge_resource_claim(&nodes, retried_release).await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(20))
        .await?;

    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Completed
    );
    let cancelled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert_eq!(
        cancelled.cleanup_command_id.map(|id| id.as_uuid()),
        Some(cleanup_lease.commands[0].command_id)
    );
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(
        resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(deployment.id.as_uuid()),
            )
            .await?
            .state,
        crate::modules::workloads::domain::entities::ResourceClaimState::Released
    );
    Ok(())
}
