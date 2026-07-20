use super::{GetWorkloadLogs, GetWorkloadLogsHandler};
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, NodeLogBatchReceiptDraft, NodeLogChunkMetadata, NodeLogChunkQuery,
    RuntimeObservationRecord,
};
use crate::modules::fleet::domain::services::{
    ILogChunkStore, LogChunkStoreError, RetrievedLogChunk, StoredLogChunk,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, IdempotentWrite, NodeCommandId, NodeId,
    OperationId, OrganizationId, ProjectId, RepositoryError, ResourceName, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::application::{WorkloadLogGapReason, WorkloadLogRecord};
use crate::modules::workloads::domain::entities::{
    Deployment, HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources,
    ServiceTemplate, Workload, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeGatewayAck,
    NodeGatewayAckReceipt, NodeLogChunkReceipt, NodeLogChunkReport, NodeObservationBatch,
    NodeObservationReceipt,
};
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogStream};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

struct LogMetadataRepository {
    chunks: Vec<NodeLogChunkMetadata>,
    calls: AtomicUsize,
}

#[async_trait]
impl INodeControlRepository for LogMetadataRepository {
    async fn enqueue_command(
        &self,
        _draft: NodeCommandDraft,
    ) -> Result<IdempotentWrite<NodeCommand>, RepositoryError> {
        unexpected()
    }

    async fn find_command(
        &self,
        _node_id: NodeId,
        _command_id: NodeCommandId,
    ) -> Result<Option<NodeCommand>, RepositoryError> {
        unexpected()
    }

    async fn lease_commands(
        &self,
        _request: &NodeCommandLeaseRequest,
        _lease_id: Uuid,
        _now: DateTime<Utc>,
        _leased_until: DateTime<Utc>,
    ) -> Result<NodeCommandLeaseResponse, RepositoryError> {
        unexpected()
    }

    async fn acknowledge_command(
        &self,
        _acknowledgement: NodeCommandAck,
        _received_at: DateTime<Utc>,
    ) -> Result<IdempotentWrite<NodeCommandAck>, RepositoryError> {
        unexpected()
    }

    async fn command_acknowledgement(
        &self,
        _node_id: NodeId,
        _command_id: NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError> {
        unexpected()
    }

    async fn record_observations(
        &self,
        _batch: NodeObservationBatch,
        _received_at: DateTime<Utc>,
    ) -> Result<NodeObservationReceipt, RepositoryError> {
        unexpected()
    }

    async fn latest_runtime_observation(
        &self,
        _node_id: NodeId,
        _unit_id: &str,
        _generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError> {
        unexpected()
    }

    async fn record_gateway_acknowledgement(
        &self,
        _acknowledgement: NodeGatewayAck,
        _received_at: DateTime<Utc>,
    ) -> Result<NodeGatewayAckReceipt, RepositoryError> {
        unexpected()
    }

    async fn record_log_chunks(
        &self,
        _batch: NodeLogBatchReceiptDraft,
        _received_at: DateTime<Utc>,
    ) -> Result<NodeLogChunkReceipt, RepositoryError> {
        unexpected()
    }

    async fn list_log_chunks(
        &self,
        query: NodeLogChunkQuery,
    ) -> Result<Vec<NodeLogChunkMetadata>, RepositoryError> {
        query.validate().map_err(RepositoryError::Conflict)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .chunks
            .iter()
            .filter(|chunk| {
                chunk.node_id == query.node_id
                    && chunk.unit_id == query.unit_id
                    && chunk.generation == query.generation
                    && chunk.sequence > query.after_sequence
                    && query.stream.is_none_or(|stream| stream == chunk.stream)
            })
            .take(query.limit)
            .cloned()
            .collect())
    }
}

struct QueryLogStore {
    objects: RwLock<BTreeMap<String, RetrievedLogChunk>>,
}

#[async_trait]
impl ILogChunkStore for QueryLogStore {
    async fn put(
        &self,
        _batch_id: Uuid,
        _node_id: Uuid,
        _ordinal: u16,
        _report: &NodeLogChunkReport,
    ) -> Result<StoredLogChunk, LogChunkStoreError> {
        Err(LogChunkStoreError::Unavailable(
            "unexpected test put".into(),
        ))
    }

    async fn get(
        &self,
        object_key: &str,
        _expected_checksum: &str,
    ) -> Result<RetrievedLogChunk, LogChunkStoreError> {
        Ok(self
            .objects
            .read()
            .await
            .get(object_key)
            .cloned()
            .unwrap_or(RetrievedLogChunk::Missing))
    }

    async fn remove(&self, _object_key: &str) -> Result<(), LogChunkStoreError> {
        Err(LogChunkStoreError::Unavailable(
            "unexpected test remove".into(),
        ))
    }

    async fn health(&self) -> Result<bool, LogChunkStoreError> {
        Ok(true)
    }
}

struct SeededWorkload {
    repository: Arc<InMemoryWorkloadRepository>,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    node_id: NodeId,
    unit_id: String,
}

#[tokio::test]
async fn workload_logs_page_by_sequence_and_surface_missing_and_corrupt_objects() {
    let seeded = seed_workload().await;
    let reports = [
        report(&seeded.unit_id, 1),
        report(&seeded.unit_id, 2),
        report(&seeded.unit_id, 3),
    ];
    let chunks = reports
        .iter()
        .enumerate()
        .map(|(index, report)| metadata(seeded.node_id, format!("object-{index}"), report))
        .collect::<Vec<_>>();
    let metadata = Arc::new(LogMetadataRepository {
        chunks,
        calls: AtomicUsize::new(0),
    });
    let objects = Arc::new(QueryLogStore {
        objects: RwLock::new(BTreeMap::from([
            (
                "object-0".into(),
                RetrievedLogChunk::Found(reports[0].clone()),
            ),
            ("object-1".into(), RetrievedLogChunk::Missing),
            ("object-2".into(), RetrievedLogChunk::Corrupt),
        ])),
    });
    let handler = GetWorkloadLogsHandler::new(seeded.repository.clone(), metadata.clone(), objects);

    let first = handler
        .execute(query(&seeded, seeded.organization_id, 0, 2), context())
        .await
        .expect("framework result")
        .expect("first log page");
    assert_eq!(first.node_id, Some(seeded.node_id));
    assert_eq!(first.next_after_sequence, Some(2));
    assert!(matches!(
        &first.records[0],
        WorkloadLogRecord::Data(chunk) if chunk.data == "line 1\n"
    ));
    assert!(matches!(
        &first.records[1],
        WorkloadLogRecord::Gap {
            reason: WorkloadLogGapReason::Missing,
            metadata,
        } if metadata.sequence == 2
    ));

    let second = handler
        .execute(query(&seeded, seeded.organization_id, 2, 2), context())
        .await
        .expect("framework result")
        .expect("second log page");
    assert_eq!(second.next_after_sequence, None);
    assert!(matches!(
        &second.records[..],
        [WorkloadLogRecord::Gap {
            reason: WorkloadLogGapReason::Corrupt,
            metadata,
        }] if metadata.sequence == 3
    ));
}

#[tokio::test]
async fn workload_log_query_does_not_cross_the_organization_boundary() {
    let seeded = seed_workload().await;
    let metadata = Arc::new(LogMetadataRepository {
        chunks: Vec::new(),
        calls: AtomicUsize::new(0),
    });
    let handler = GetWorkloadLogsHandler::new(
        seeded.repository.clone(),
        metadata.clone(),
        Arc::new(QueryLogStore {
            objects: RwLock::new(BTreeMap::new()),
        }),
    );
    let result = handler
        .execute(query(&seeded, OrganizationId::new(), 0, 10), context())
        .await
        .expect("framework result");
    assert!(matches!(result, Err(ApplicationError::NotFound(_))));
    assert_eq!(metadata.calls.load(Ordering::SeqCst), 0);
}

fn query(
    seeded: &SeededWorkload,
    organization_id: OrganizationId,
    after_sequence: u64,
    limit: u16,
) -> GetWorkloadLogs {
    GetWorkloadLogs {
        organization_id,
        workload_id: seeded.workload_id,
        revision_id: seeded.revision_id,
        after_sequence,
        limit,
        stream: None,
    }
}

fn report(unit_id: &str, sequence: u64) -> NodeLogChunkReport {
    let data = format!("line {sequence}\n");
    NodeLogChunkReport {
        unit_id: unit_id.into(),
        generation: 1,
        chunk: RuntimeLogChunk {
            schema: RuntimeLogChunk::SCHEMA.into(),
            cursor: format!("source:{sequence}"),
            sequence,
            observed_at_ms: sequence,
            stream: RuntimeLogStream::Stdout,
            data: data.clone(),
        },
        checksum: format!("sha256:{:x}", Sha256::digest(data.as_bytes())),
    }
}

fn metadata(
    node_id: NodeId,
    object_key: String,
    report: &NodeLogChunkReport,
) -> NodeLogChunkMetadata {
    NodeLogChunkMetadata {
        node_id,
        unit_id: report.unit_id.clone(),
        generation: report.generation,
        cursor: report.chunk.cursor.clone(),
        sequence: report.chunk.sequence,
        observed_at_ms: report.chunk.observed_at_ms,
        stream: report.chunk.stream,
        checksum: report.checksum.clone(),
        object_key,
    }
}

async fn seed_workload() -> SeededWorkload {
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("log-test").expect("workload name"),
        now,
    );
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        service_template(),
        now,
    )
    .expect("workload revision");
    let deployment = Deployment::create(
        DeploymentId::new(),
        organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        now,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid()).expect("operation subject"),
        WorkflowIdentity::new("cloud.deployment", "1").expect("workflow identity"),
        serde_json::json!({}),
        now,
    );
    let event =
        DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7()).expect("event");
    repository
        .create_deployment(CreateDeploymentBundle {
            workload: workload.clone(),
            revision: revision.clone(),
            deployment: deployment.clone(),
            operation,
            idempotency: IdempotencyRequest::new("test", "log-test", b"log-test")
                .expect("idempotency"),
            event,
        })
        .await
        .expect("create deployment");
    let resolving = repository
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            now + Duration::seconds(1),
        )
        .await
        .expect("resolve deployment");
    let node_id = NodeId::new();
    repository
        .assign_node(
            deployment.id,
            resolving.aggregate_version,
            node_id,
            now + Duration::seconds(2),
        )
        .await
        .expect("assign node");
    SeededWorkload {
        repository,
        organization_id,
        workload_id: workload.id,
        revision_id: revision.id,
        node_id,
        unit_id: revision.runtime_unit_id(),
    }
}

fn service_template() -> ServiceTemplate {
    let digest = format!("sha256:{}", "a".repeat(64));
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/cloud/log-test@{digest}"),
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
        health: HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5_000,
        },
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn unexpected<T>() -> Result<T, RepositoryError> {
    Err(RepositoryError::Storage(
        "unexpected test repository operation".into(),
    ))
}
