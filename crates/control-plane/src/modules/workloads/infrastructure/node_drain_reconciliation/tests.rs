use super::*;
use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::{NodeCapabilities, NodeName, NodeState};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, OperationId, OrganizationId, ProjectId,
    ResourceName, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources,
    ServiceTemplate, Workload, WorkloadControlSpec, WorkloadReplicaLifecycle, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
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
            NodeEvacuationCause::PoolMaintenance { .. } => true,
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
            NodeEvacuationCause::PoolMaintenance { .. } => true,
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
        repository.clone(),
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
        WorkflowIdentity::new("cloud.deployment", "3")?,
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
