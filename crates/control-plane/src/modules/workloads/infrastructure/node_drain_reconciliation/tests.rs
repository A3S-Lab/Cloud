use super::*;
use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::{NodeCapabilities, NodeName};
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

struct FakeDrainNodes {
    node: RwLock<Node>,
}

#[async_trait]
impl INodeDrainRepository for FakeDrainNodes {
    async fn list_draining(&self, limit: usize) -> Result<Vec<Node>, RepositoryError> {
        if limit == 0 {
            return Err(RepositoryError::Conflict("limit is zero".into()));
        }
        let node = self.node.read().await.clone();
        Ok((node.state == NodeState::Draining)
            .then_some(node)
            .into_iter()
            .take(limit)
            .collect())
    }

    async fn find_drain_node(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<Node, RepositoryError> {
        let node = self.node.read().await.clone();
        if node.organization_id == organization_id && node.id == node_id {
            Ok(node)
        } else {
            Err(RepositoryError::NotFound)
        }
    }
}

#[tokio::test]
async fn draining_node_requests_one_generation_fenced_evacuation(
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
    node.drain()?;
    let nodes = Arc::new(FakeDrainNodes {
        node: RwLock::new(node),
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
    assert_eq!(report.draining_nodes, 1);
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
