use super::*;
use crate::modules::fleet::domain::entities::{Node, NodePool};
use crate::modules::fleet::domain::value_objects::{NodeCapabilities, NodeName, NodeState};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, IdempotentWrite, OperationId, OrganizationId,
    ProjectId, ResourceClaimId, ResourceName, WorkloadId, WorkloadReplicaId,
    WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, HttpHealthCheck, OciArtifact, ResourceAllocation,
    ResourceClaimReservation, ResourceKind, ResourceSlotRequest, ResourceUnit, ServicePort,
    ServiceProcess, ServiceResources, ServiceTemplate, Workload, WorkloadControlSpec,
    WorkloadReplicaLifecycle, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IResourceClaimRepository, IWorkloadRepository,
};
use crate::modules::workloads::infrastructure::{
    InMemoryResourceClaimRepository, InMemoryWorkloadRepository,
};
use a3s_cloud_contracts::{NodeResourceInventory, NodeResourceSlot};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use serde_json::json;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[test]
fn manual_drain_correlation_id_remains_backward_compatible() {
    let replica_id = crate::modules::shared_kernel::domain::WorkloadReplicaId::new();
    let source_node_id = NodeId::new();
    let candidate = ReplicaEvacuationCandidate {
        organization_id: OrganizationId::new(),
        workload_id: WorkloadId::new(),
        replica_id,
        replica_generation: 11,
        expected_replica_version: 3,
        member_id: crate::modules::shared_kernel::domain::WorkloadReplicaMemberId::new(),
        expected_member_version: 2,
        source_node_id,
        placement_generation: 7,
    };
    let legacy_identity = format!(
        "{EVACUATION_CORRELATION_DOMAIN}:{}:{}",
        candidate.replica_generation, source_node_id
    );

    assert_eq!(
        evacuation_correlation_id(candidate, &NodeEvacuationCause::ManualDrain),
        Uuid::new_v5(&replica_id.as_uuid(), legacy_identity.as_bytes())
    );
}

struct FakeDrainNodes {
    node: RwLock<Node>,
    cause: NodeEvacuationCause,
}

#[async_trait]
impl INodeDrainRepository for FakeDrainNodes {
    async fn list_evacuation_sources(
        &self,
        _evaluated_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<NodeEvacuationSource>, RepositoryError> {
        if limit == 0 {
            return Err(RepositoryError::Conflict("limit is zero".into()));
        }
        let node = self.node.read().await.clone();
        let eligible = match self.cause {
            NodeEvacuationCause::ManualDrain => node.state == NodeState::Draining,
            NodeEvacuationCause::PoolMaintenance { .. }
            | NodeEvacuationCause::PoolMemberRemoval { .. } => true,
        };
        Ok(eligible
            .then_some(NodeEvacuationSource {
                node,
                cause: self.cause.clone(),
            })
            .into_iter()
            .take(limit)
            .collect())
    }

    async fn find_evacuation_source(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        _evaluated_at: DateTime<Utc>,
    ) -> Result<NodeEvacuationSource, RepositoryError> {
        let node = self.node.read().await.clone();
        let eligible = match self.cause {
            NodeEvacuationCause::ManualDrain => node.state == NodeState::Draining,
            NodeEvacuationCause::PoolMaintenance { .. }
            | NodeEvacuationCause::PoolMemberRemoval { .. } => true,
        };
        if node.organization_id == organization_id && node.id == node_id && eligible {
            Ok(NodeEvacuationSource {
                node,
                cause: self.cause.clone(),
            })
        } else {
            Err(RepositoryError::NotFound)
        }
    }
}

struct UnusedNodePools;

#[async_trait]
impl INodePoolRepository for UnusedNodePools {
    async fn replay(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<crate::modules::fleet::domain::entities::NodePool>, RepositoryError> {
        Ok(None)
    }

    async fn save(
        &self,
        _write: NodePoolWrite,
    ) -> Result<IdempotentWrite<crate::modules::fleet::domain::entities::NodePool>, RepositoryError>
    {
        Err(RepositoryError::Storage("unused node pool save".into()))
    }

    async fn find(
        &self,
        _organization_id: OrganizationId,
        _pool_id: NodePoolId,
    ) -> Result<crate::modules::fleet::domain::entities::NodePool, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
    ) -> Result<Vec<crate::modules::fleet::domain::entities::NodePool>, RepositoryError> {
        Ok(Vec::new())
    }
}

struct FakeNodePools {
    pool: RwLock<NodePool>,
}

#[async_trait]
impl INodePoolRepository for FakeNodePools {
    async fn replay(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<NodePool>, RepositoryError> {
        Ok(None)
    }

    async fn save(
        &self,
        write: NodePoolWrite,
    ) -> Result<IdempotentWrite<NodePool>, RepositoryError> {
        let mut current = self.pool.write().await;
        let expected_version = write
            .expected_version
            .ok_or_else(|| RepositoryError::Conflict("test node pool already exists".into()))?;
        write
            .pool
            .validate_successor(&current, expected_version)
            .map_err(RepositoryError::Conflict)?;
        *current = write.pool.clone();
        Ok(IdempotentWrite {
            value: write.pool,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        pool_id: NodePoolId,
    ) -> Result<NodePool, RepositoryError> {
        let pool = self.pool.read().await;
        if pool.organization_id == organization_id && pool.id == pool_id {
            Ok(pool.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<NodePool>, RepositoryError> {
        let pool = self.pool.read().await;
        Ok((pool.organization_id == organization_id)
            .then(|| pool.clone())
            .into_iter()
            .collect())
    }
}

#[tokio::test]
async fn maintenance_target_requests_one_generation_fenced_evacuation(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now() - ChronoDuration::minutes(1);
    let organization_id = OrganizationId::new();
    let source_node_id = NodeId::new();
    let mut node = Node::enroll(
        source_node_id,
        organization_id,
        NodeName::new("draining-source")?,
        Uuid::now_v7(),
        "test-agent",
        NodeCapabilities::new("test", "1", json!({}))?,
        now,
    )?;
    node.mark_ready()?;
    let nodes = Arc::new(FakeDrainNodes {
        node: RwLock::new(node),
        cause: NodeEvacuationCause::PoolMaintenance {
            pool_id: crate::modules::shared_kernel::domain::NodePoolId::new(),
            generation: 7,
            ends_at: now + ChronoDuration::hours(1),
        },
    });
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("node drain fixture")?,
        now,
    );
    let bundle = deployment_bundle(workload.clone(), now)?;
    let deployment = bundle.deployment.clone();
    repository.create_deployment(bundle).await?;
    let resolving = repository
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            now + ChronoDuration::seconds(1),
        )
        .await?;
    repository
        .assign_node(
            resolving.id,
            resolving.aggregate_version,
            source_node_id,
            now + ChronoDuration::seconds(2),
        )
        .await?;

    let reconciler = NodeDrainEvacuationReconciler::new(
        nodes,
        Arc::new(UnusedNodePools),
        repository.clone(),
        Arc::new(InMemoryResourceClaimRepository::new()),
        Duration::from_secs(1),
        10,
        10,
    )?;
    let report = reconciler
        .run_once(now + ChronoDuration::seconds(3))
        .await?;
    assert_eq!(report.source_nodes, 1);
    assert_eq!(report.manual_drain_nodes, 0);
    assert_eq!(report.maintenance_nodes, 1);
    assert_eq!(report.candidates, 1);
    assert_eq!(report.requested, 1);
    assert!(report.failures.is_empty());

    let replica = repository
        .list_workload_replicas(organization_id, workload.id)
        .await?
        .into_iter()
        .next()
        .ok_or("replica")?;
    assert_eq!(replica.lifecycle, WorkloadReplicaLifecycle::Retiring);
    assert_eq!(replica.evacuation_node_id, Some(source_node_id));
    assert_eq!(replica.retirement_command_id, None);
    assert_eq!(replica.runtime_fenced_at, None);
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.replica.evacuation.requested")
            .count(),
        1
    );

    let replay = reconciler
        .run_once(now + ChronoDuration::seconds(4))
        .await?;
    assert_eq!(replay.candidates, 0);
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.replica.evacuation.requested")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn member_removal_waits_for_durable_replica_placement_cleanup(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now() - ChronoDuration::minutes(1);
    let organization_id = OrganizationId::new();
    let source_node_id = NodeId::new();
    let retained_node_id = NodeId::new();
    let pool_id = NodePoolId::new();
    let mut node = Node::enroll(
        source_node_id,
        organization_id,
        NodeName::new("removal-source")?,
        Uuid::now_v7(),
        "test-agent",
        NodeCapabilities::new("test", "1", json!({}))?,
        now,
    )?;
    node.mark_ready()?;
    let nodes = Arc::new(FakeDrainNodes {
        node: RwLock::new(node),
        cause: NodeEvacuationCause::PoolMemberRemoval {
            pool_id,
            generation: 1,
        },
    });
    let mut pool = NodePool::create(
        pool_id,
        organization_id,
        ResourceName::parse("removal workers")?,
        vec![source_node_id, retained_node_id],
        now,
    )?;
    pool.request_member_removal(vec![source_node_id], now + ChronoDuration::seconds(1))?;
    let pools = Arc::new(FakeNodePools {
        pool: RwLock::new(pool),
    });
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("member removal fixture")?,
        now,
    );
    let bundle = deployment_bundle(workload, now)?;
    let deployment = bundle.deployment.clone();
    repository.create_deployment(bundle).await?;
    let resolving = repository
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            now + ChronoDuration::seconds(1),
        )
        .await?;
    repository
        .assign_node(
            resolving.id,
            resolving.aggregate_version,
            source_node_id,
            now + ChronoDuration::seconds(2),
        )
        .await?;
    let reconciler = NodeDrainEvacuationReconciler::new(
        nodes,
        pools.clone(),
        repository,
        Arc::new(InMemoryResourceClaimRepository::new()),
        Duration::from_secs(1),
        10,
        10,
    )?;

    let report = reconciler
        .run_once(now + ChronoDuration::seconds(3))
        .await?;
    assert_eq!(report.member_removal_nodes, 1);
    assert_eq!(report.requested, 1);
    assert_eq!(report.member_removals_completed, 0);
    assert!(report.failures.is_empty());
    let current = pools.pool.read().await;
    assert!(current.member_node_ids.contains(&source_node_id));
    assert!(current.member_removal(source_node_id).is_some());
    Ok(())
}

#[tokio::test]
async fn member_removal_completes_only_after_the_node_has_no_replica_placement(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now() - ChronoDuration::minutes(1);
    let organization_id = OrganizationId::new();
    let source_node_id = NodeId::new();
    let retained_node_id = NodeId::new();
    let pool_id = NodePoolId::new();
    let mut node = Node::enroll(
        source_node_id,
        organization_id,
        NodeName::new("empty-removal-source")?,
        Uuid::now_v7(),
        "test-agent",
        NodeCapabilities::new("test", "1", json!({}))?,
        now,
    )?;
    node.mark_ready()?;
    let nodes = Arc::new(FakeDrainNodes {
        node: RwLock::new(node),
        cause: NodeEvacuationCause::PoolMemberRemoval {
            pool_id,
            generation: 1,
        },
    });
    let mut pool = NodePool::create(
        pool_id,
        organization_id,
        ResourceName::parse("empty removal workers")?,
        vec![source_node_id, retained_node_id],
        now,
    )?;
    pool.request_member_removal(vec![source_node_id], now + ChronoDuration::seconds(1))?;
    let pools = Arc::new(FakeNodePools {
        pool: RwLock::new(pool),
    });
    let reconciler = NodeDrainEvacuationReconciler::new(
        nodes,
        pools.clone(),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemoryResourceClaimRepository::new()),
        Duration::from_secs(1),
        10,
        10,
    )?;

    let report = reconciler
        .run_once(now + ChronoDuration::seconds(2))
        .await?;
    assert_eq!(report.member_removal_nodes, 1);
    assert_eq!(report.candidates, 0);
    assert_eq!(report.member_removals_completed, 1);
    assert!(report.failures.is_empty());
    let current = pools.pool.read().await;
    assert_eq!(current.member_node_ids, vec![retained_node_id]);
    assert!(current.member_removal(source_node_id).is_none());
    assert_eq!(current.aggregate_version, 3);
    Ok(())
}

#[tokio::test]
async fn member_removal_waits_for_active_resource_claim_release(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now() - ChronoDuration::minutes(1);
    let organization_id = OrganizationId::new();
    let source_node_id = NodeId::new();
    let retained_node_id = NodeId::new();
    let pool_id = NodePoolId::new();
    let mut node = Node::enroll(
        source_node_id,
        organization_id,
        NodeName::new("claimed-removal-source")?,
        Uuid::now_v7(),
        "test-agent",
        NodeCapabilities::new("test", "1", json!({}))?,
        now,
    )?;
    node.mark_ready()?;
    let nodes = Arc::new(FakeDrainNodes {
        node: RwLock::new(node),
        cause: NodeEvacuationCause::PoolMemberRemoval {
            pool_id,
            generation: 1,
        },
    });
    let mut pool = NodePool::create(
        pool_id,
        organization_id,
        ResourceName::parse("claimed removal workers")?,
        vec![source_node_id, retained_node_id],
        now,
    )?;
    pool.request_member_removal(vec![source_node_id], now + ChronoDuration::seconds(1))?;
    let pools = Arc::new(FakeNodePools {
        pool: RwLock::new(pool),
    });
    let claims = Arc::new(InMemoryResourceClaimRepository::new());
    let claim = claims
        .reserve(resource_claim_reservation(
            organization_id,
            source_node_id,
            now + ChronoDuration::seconds(2),
        ))
        .await?
        .value;
    let reconciler = NodeDrainEvacuationReconciler::new(
        nodes,
        pools.clone(),
        Arc::new(InMemoryWorkloadRepository::new()),
        claims.clone(),
        Duration::from_secs(1),
        10,
        10,
    )?;

    let blocked = reconciler
        .run_once(now + ChronoDuration::seconds(3))
        .await?;
    assert_eq!(blocked.member_removals_completed, 0);
    assert!(blocked.failures.is_empty());
    assert!(pools
        .pool
        .read()
        .await
        .member_removal(source_node_id)
        .is_some());

    claims
        .cancel_database_reservation(
            organization_id,
            claim.id,
            claim.aggregate_version,
            now + ChronoDuration::seconds(4),
        )
        .await?;
    let completed = reconciler
        .run_once(now + ChronoDuration::seconds(5))
        .await?;
    assert_eq!(completed.member_removals_completed, 1);
    assert!(completed.failures.is_empty());
    assert_eq!(
        pools.pool.read().await.member_node_ids,
        vec![retained_node_id]
    );
    Ok(())
}

fn deployment_bundle(
    workload: Workload,
    requested_at: DateTime<Utc>,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        service_template(),
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
        WorkflowIdentity::new("cloud.deployment", "4")?,
        json!({
            "deploymentId": deployment.id,
            "organizationId": workload.organization_id,
            "revisionId": revision.id,
            "workloadId": workload.id,
        }),
        requested_at,
    );
    let event = DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?;
    Ok(CreateDeploymentBundle {
        workload,
        control: WorkloadControlSpec::unmanaged_replica_set(1, 1)?,
        revision,
        deployment,
        operation,
        idempotency: IdempotencyRequest::new(
            "test.node-drain",
            Uuid::now_v7().to_string(),
            b"node-drain-fixture",
        )?,
        event,
    })
}

fn service_template() -> ServiceTemplate {
    let digest = format!("sha256:{}", "a".repeat(64));
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/cloud/drain@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/fixture".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: Some(HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 1_000,
        }),
    }
}

fn resource_claim_reservation(
    organization_id: OrganizationId,
    node_id: NodeId,
    reserved_at: DateTime<Utc>,
) -> ResourceClaimReservation {
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let allocation = ResourceAllocation::Scalar {
        amount: 1,
        unit: ResourceUnit::Count,
    };
    ResourceClaimReservation {
        id: ResourceClaimId::new(),
        binding: DeploymentReplicaBinding {
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
            created_at: reserved_at,
            updated_at: reserved_at,
        },
        node_id,
        inventory: NodeResourceInventory::new(
            node_id.as_uuid(),
            Uuid::now_v7(),
            1,
            reserved_at,
            vec![NodeResourceSlot::new(
                ResourceKind::Accelerator,
                "accelerator/claim-fence",
                allocation.clone(),
            )
            .expect("resource inventory slot")],
        )
        .expect("resource inventory"),
        topology_digest: format!("sha256:{}", "b".repeat(64)),
        slots: vec![ResourceSlotRequest::new(
            ResourceKind::Accelerator,
            "accelerator/claim-fence",
            allocation,
        )
        .expect("resource slot request")],
        reserved_at,
    }
}
