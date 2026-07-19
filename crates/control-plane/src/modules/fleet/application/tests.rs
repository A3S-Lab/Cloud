use super::{
    AcknowledgeNodeCommand, AcknowledgeNodeCommandHandler, ChangeNodeState, ChangeNodeStateHandler,
    EnqueueNodeCommand, EnqueueNodeCommandHandler, EnrollNode, EnrollNodeHandler, GetNode,
    GetNodeHandler, IssueEnrollmentToken, IssueEnrollmentTokenHandler, LeaseNodeCommands,
    LeaseNodeCommandsHandler, ListNodes, ListNodesHandler, LogCompactionWorker, LogRetentionWorker,
    RecordNodeLogChunks, RecordNodeLogChunksHandler, RotateNodeCertificate,
    RotateNodeCertificateHandler,
};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeHeartbeatUpdate, NodeLogChunkQuery,
};
use crate::modules::fleet::domain::services::{ILogChunkStore, RetrievedLogChunk};
use crate::modules::fleet::domain::value_objects::{NodeCapabilities, NodeState};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::fleet::infrastructure::{LocalCertificateAuthority, LocalLogChunkStore};
use crate::modules::identity::domain::entities::Organization;
use crate::modules::identity::domain::events::OrganizationCreated;
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::identity::domain::value_objects::OrganizationName;
use crate::modules::identity::infrastructure::persistence::InMemoryIdentityRepository;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, NodeCertificateId, NodeCommandId, NodeId, OrganizationId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult, NodeEnrollmentRequest, NodeEnrollmentResponse, NodeLogChunkBatch,
    NodeLogChunkReport,
};
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeFeature,
    RuntimeLogChunk, RuntimeLogStream, RuntimeUnitClass,
};
use chrono::{Duration, Utc};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use uuid::Uuid;

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("docker").expect("valid Docker provider ID"),
        provider_build: "docker-test".into(),
        unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::None, NetworkMode::Service],
        mount_kinds: Vec::new(),
        health_check_kinds: Vec::new(),
        resource_controls: vec![
            ResourceControl::Cpu,
            ResourceControl::Memory,
            ResourceControl::Pids,
            ResourceControl::EphemeralStorage,
            ResourceControl::ExecutionTimeout,
        ],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
        ],
    }
}

fn csr() -> String {
    let key = KeyPair::generate().expect("node private key");
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "node-agent");
    params.distinguished_name = name;
    params
        .serialize_request(&key)
        .expect("CSR")
        .pem()
        .expect("CSR PEM")
}

async fn organization(repository: &InMemoryIdentityRepository) -> Organization {
    let now = Utc::now();
    let organization = Organization::create(
        OrganizationId::new(),
        OrganizationName::parse("Acme").expect("organization name"),
        now,
    );
    IOrganizationRepository::create(
        repository,
        organization.clone(),
        OrganizationCreated::envelope(&organization, Uuid::now_v7()).expect("event"),
        IdempotencyRequest::new("organizations", "acme", b"Acme").expect("idempotency"),
    )
    .await
    .expect("create organization");
    organization
}

#[tokio::test]
async fn enrollment_rotation_state_and_offline_projection_form_a_replay_safe_flow() {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let organization = organization(&identity).await;
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let ca_directory = tempfile::tempdir().expect("CA directory");
    let certificate_authority =
        Arc::new(LocalCertificateAuthority::load_or_create(ca_directory.path()).expect("local CA"));
    let now = Utc::now();
    let token_secret = format!("a3sn_{}", "a".repeat(64));
    let issue = IssueEnrollmentToken {
        organization_id: organization.id,
        name: "primary worker".into(),
        token_secret: token_secret.clone(),
        expires_at: now + Duration::minutes(10),
        idempotency_key: "issue-worker".into(),
        request_id: Uuid::now_v7(),
        requested_at: now,
    };
    let issue_handler = IssueEnrollmentTokenHandler::new(identity.clone(), nodes.clone());
    let issued = issue_handler
        .execute(issue.clone(), context())
        .await
        .expect("framework result")
        .expect("issue token");
    assert!(!issued.replayed);
    let replayed_issue = issue_handler
        .execute(issue, context())
        .await
        .expect("framework result")
        .expect("issue replay");
    assert!(replayed_issue.replayed);
    assert_eq!(
        replayed_issue.enrollment_token.id,
        issued.enrollment_token.id
    );

    let enrollment_request = NodeEnrollmentRequest {
        schema: NodeEnrollmentRequest::SCHEMA.into(),
        enrollment_token: token_secret,
        node_name: "worker-1".into(),
        agent_instance_id: Uuid::now_v7(),
        agent_version: "0.1.0".into(),
        csr_pem: csr(),
        runtime_capabilities: capabilities(),
    };
    let enroll_handler = EnrollNodeHandler::new(
        nodes.clone(),
        certificate_authority.clone(),
        Duration::hours(1),
        15 * 60 * 1_000,
        5_000,
        15_000,
    )
    .expect("enrollment handler");
    let enroll = EnrollNode {
        request: enrollment_request.clone(),
        request_id: Uuid::now_v7(),
        received_at: now + Duration::seconds(1),
    };
    let enrolled = enroll_handler
        .execute(enroll.clone(), context())
        .await
        .expect("framework result")
        .expect("enroll node");
    assert!(!enrolled.replayed);
    enrolled.response.validate().expect("enrollment response");
    let enrollment_replay = enroll_handler
        .execute(
            EnrollNode {
                received_at: now + Duration::seconds(2),
                ..enroll
            },
            context(),
        )
        .await
        .expect("framework result")
        .expect("enrollment replay");
    assert!(enrollment_replay.replayed);
    assert_eq!(enrollment_replay.response, enrolled.response);

    let node_id = NodeId::from_uuid(enrolled.response.node_id);
    let runtime_capabilities = capabilities();
    let node_capabilities = NodeCapabilities::new(
        runtime_capabilities.provider_id.to_string(),
        runtime_capabilities.provider_build.clone(),
        serde_json::to_value(runtime_capabilities).expect("capability document"),
    )
    .expect("node capabilities");
    let ready = nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id,
            agent_instance_id: enrollment_request.agent_instance_id,
            agent_version: enrollment_request.agent_version,
            capabilities: node_capabilities,
            observed_at: now + Duration::seconds(3),
        })
        .await
        .expect("heartbeat");
    assert_eq!(ready.state, NodeState::Ready);

    let enqueue_handler = EnqueueNodeCommandHandler::new(nodes.clone());
    let issued_at = now + Duration::seconds(4);
    let enqueued = enqueue_handler
        .execute(
            EnqueueNodeCommand {
                draft: NodeCommandDraft {
                    proposed_command_id: NodeCommandId::new(),
                    node_id,
                    aggregate_id: Uuid::now_v7(),
                    payload: NodeCommandPayload::RuntimeInspect {
                        unit_id: "worker-service".into(),
                        generation: 1,
                    },
                    issued_at,
                    not_after: issued_at + Duration::minutes(1),
                    correlation_id: Uuid::now_v7(),
                },
            },
            context(),
        )
        .await
        .expect("framework result")
        .expect("enqueue command");
    assert_eq!(enqueued.command.sequence, 1);

    let lease_handler = LeaseNodeCommandsHandler::new(
        nodes.clone(),
        Duration::seconds(10),
        StdDuration::from_secs(15),
        StdDuration::from_millis(25),
    )
    .expect("lease handler");
    let lease_request = NodeCommandLeaseRequest {
        schema: NodeCommandLeaseRequest::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        agent_instance_id: enrollment_request.agent_instance_id,
        after_sequence: 0,
        max_commands: 1,
        wait_ms: 0,
    };
    assert!(lease_handler
        .execute(
            LeaseNodeCommands {
                authenticated_node_id: NodeId::new(),
                request: lease_request.clone(),
                received_at: issued_at,
            },
            context(),
        )
        .await
        .expect("framework result")
        .is_err());
    let lease = lease_handler
        .execute(
            LeaseNodeCommands {
                authenticated_node_id: node_id,
                request: lease_request,
                received_at: issued_at,
            },
            context(),
        )
        .await
        .expect("framework result")
        .expect("lease command");
    assert_eq!(lease.commands.len(), 1);
    let leased = &lease.commands[0];
    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: leased.command_id,
        lease_id: leased.lease_id,
        node_id: leased.node_id,
        sequence: leased.sequence,
        payload_digest: leased.payload_digest.clone(),
        completed_at: issued_at + Duration::seconds(1),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeInspected {
                inspection: a3s_runtime::contract::RuntimeInspection::NotFound {
                    schema: a3s_runtime::contract::RuntimeInspection::SCHEMA.into(),
                    unit_id: "worker-service".into(),
                    last_generation: Some(1),
                },
            }),
        },
    };
    let acknowledgement_handler = AcknowledgeNodeCommandHandler::new(nodes.clone());
    let acknowledged = acknowledgement_handler
        .execute(
            AcknowledgeNodeCommand {
                authenticated_node_id: node_id,
                acknowledgement: acknowledgement.clone(),
                received_at: issued_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("framework result")
        .expect("acknowledge command");
    assert!(!acknowledged.replayed);
    assert!(
        acknowledgement_handler
            .execute(
                AcknowledgeNodeCommand {
                    authenticated_node_id: node_id,
                    acknowledgement,
                    received_at: issued_at + Duration::seconds(2),
                },
                context(),
            )
            .await
            .expect("framework result")
            .expect("replay acknowledgement")
            .replayed
    );

    let log_store =
        Arc::new(LocalLogChunkStore::new(ca_directory.path().join("logs")).expect("log store"));
    let log_handler = RecordNodeLogChunksHandler::new(nodes.clone(), log_store.clone());
    let log_data = "service started";
    let log_batch = NodeLogChunkBatch {
        schema: NodeLogChunkBatch::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        sent_at: issued_at + Duration::seconds(3),
        chunks: vec![NodeLogChunkReport {
            unit_id: "worker-service".into(),
            generation: 1,
            chunk: RuntimeLogChunk {
                schema: RuntimeLogChunk::SCHEMA.into(),
                cursor: "cursor:1".into(),
                sequence: 1,
                observed_at_ms: 1,
                stream: RuntimeLogStream::Stdout,
                data: log_data.into(),
            },
            checksum: format!("sha256:{:x}", Sha256::digest(log_data.as_bytes())),
        }],
    };
    let logged = log_handler
        .execute(
            RecordNodeLogChunks {
                authenticated_node_id: node_id,
                batch: log_batch.clone(),
                received_at: issued_at + Duration::seconds(3),
            },
            context(),
        )
        .await
        .expect("framework result")
        .expect("record logs");
    assert!(!logged.replayed);
    assert!(
        log_handler
            .execute(
                RecordNodeLogChunks {
                    authenticated_node_id: node_id,
                    batch: log_batch.clone(),
                    received_at: issued_at + Duration::seconds(4),
                },
                context(),
            )
            .await
            .expect("framework result")
            .expect("replay logs")
            .replayed
    );
    let retention = LogRetentionWorker::new(
        nodes.clone(),
        log_store.clone(),
        StdDuration::from_secs(1),
        StdDuration::from_millis(10),
        16,
    )
    .expect("log retention worker");
    let retained = retention
        .run_once(issued_at + Duration::seconds(5))
        .await
        .expect("retain logs");
    assert_eq!(retained.retained, 1);
    let metadata = nodes
        .list_log_chunks(NodeLogChunkQuery {
            node_id,
            unit_id: "worker-service".into(),
            generation: 1,
            after_sequence: None,
            limit: 2,
            stream: None,
        })
        .await
        .expect("retained log metadata");
    assert!(metadata[0].retained_at.is_some());
    assert_eq!(
        log_store
            .get(&metadata[0].object_key, &metadata[0].checksum)
            .await
            .expect("retained object lookup"),
        RetrievedLogChunk::Missing
    );
    assert!(
        log_handler
            .execute(
                RecordNodeLogChunks {
                    authenticated_node_id: node_id,
                    batch: log_batch.clone(),
                    received_at: issued_at + Duration::seconds(6),
                },
                context(),
            )
            .await
            .expect("framework result")
            .expect("replay retained logs")
            .replayed
    );
    assert_eq!(
        log_store
            .get(&metadata[0].object_key, &metadata[0].checksum)
            .await
            .expect("retained replay object lookup"),
        RetrievedLogChunk::Missing
    );
    let compaction = LogCompactionWorker::new(
        nodes.clone(),
        StdDuration::from_secs(1),
        StdDuration::from_millis(10),
        16,
    )
    .expect("log compaction worker");
    let compacted = compaction
        .run_once(issued_at + Duration::seconds(7))
        .await
        .expect("compact retained logs");
    assert_eq!(compacted.compacted_tombstones, 1);
    assert_eq!(compacted.created_ranges, 1);
    assert!(nodes
        .list_log_chunks(NodeLogChunkQuery {
            node_id,
            unit_id: "worker-service".into(),
            generation: 1,
            after_sequence: None,
            limit: 2,
            stream: None,
        })
        .await
        .expect("compacted log metadata")
        .is_empty());
    let ranges = nodes
        .list_log_compaction_ranges(NodeLogChunkQuery {
            node_id,
            unit_id: "worker-service".into(),
            generation: 1,
            after_sequence: None,
            limit: 2,
            stream: None,
        })
        .await
        .expect("log compaction ranges");
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].first_sequence, 1);
    assert_eq!(ranges[0].through_sequence, 1);
    assert!(
        log_handler
            .execute(
                RecordNodeLogChunks {
                    authenticated_node_id: node_id,
                    batch: log_batch.clone(),
                    received_at: issued_at + Duration::seconds(8),
                },
                context(),
            )
            .await
            .expect("framework result")
            .expect("replay compacted logs")
            .replayed
    );
    let mut reused_sequence = log_batch;
    reused_sequence.batch_id = Uuid::now_v7();
    assert!(log_handler
        .execute(
            RecordNodeLogChunks {
                authenticated_node_id: node_id,
                batch: reused_sequence,
                received_at: issued_at + Duration::seconds(9),
            },
            context(),
        )
        .await
        .expect("framework result")
        .is_err());
    assert_eq!(
        log_store
            .get(&metadata[0].object_key, &metadata[0].checksum)
            .await
            .expect("compacted conflict object lookup"),
        RetrievedLogChunk::Missing
    );

    let rotate_handler = RotateNodeCertificateHandler::new(
        nodes.clone(),
        certificate_authority.clone(),
        Duration::hours(1),
    )
    .expect("rotation handler");
    let rotate = RotateNodeCertificate {
        organization_id: organization.id,
        node_id,
        current_certificate_id: NodeCertificateId::from_uuid(
            enrolled.response.certificate.certificate_id,
        ),
        csr_pem: csr(),
        idempotency_key: "rotate-worker".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(4),
    };
    let rotated = rotate_handler
        .execute(rotate.clone(), context())
        .await
        .expect("framework result")
        .expect("rotate certificate");
    assert!(!rotated.replayed);
    let rotation_replay = rotate_handler
        .execute(rotate, context())
        .await
        .expect("framework result")
        .expect("rotation replay");
    assert!(rotation_replay.replayed);
    assert_eq!(rotation_replay.certificate, rotated.certificate);
    let revocations =
        std::fs::read_to_string(ca_directory.path().join("revoked-serials")).expect("revocations");
    assert_eq!(revocations.lines().count(), 1);

    let current = nodes
        .find(organization.id, node_id)
        .await
        .expect("current node");
    let state_handler = ChangeNodeStateHandler::new(nodes.clone(), certificate_authority.clone());
    let drain = ChangeNodeState {
        organization_id: organization.id,
        node_id,
        state: NodeState::Draining,
        expected_version: current.aggregate_version,
        idempotency_key: "drain-worker".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(5),
    };
    let drained = state_handler
        .execute(drain.clone(), context())
        .await
        .expect("framework result")
        .expect("drain node");
    assert_eq!(drained.node.state, NodeState::Draining);
    assert!(
        state_handler
            .execute(drain, context())
            .await
            .expect("framework result")
            .expect("drain replay")
            .replayed
    );

    let revoke = ChangeNodeState {
        organization_id: organization.id,
        node_id,
        state: NodeState::Revoked,
        expected_version: drained.node.aggregate_version,
        idempotency_key: "revoke-worker".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(6),
    };
    let revoked = state_handler
        .execute(revoke.clone(), context())
        .await
        .expect("framework result")
        .expect("revoke node");
    assert_eq!(revoked.node.state, NodeState::Revoked);
    assert!(
        state_handler
            .execute(revoke, context())
            .await
            .expect("framework result")
            .expect("revoke replay")
            .replayed
    );
    let revocations =
        std::fs::read_to_string(ca_directory.path().join("revoked-serials")).expect("revocations");
    assert_eq!(revocations.lines().count(), 2);

    let get_handler = GetNodeHandler::new(nodes.clone(), Duration::seconds(20)).expect("get");
    let queried = get_handler
        .execute(
            GetNode {
                organization_id: organization.id,
                node_id,
                queried_at: now + Duration::seconds(30),
            },
            context(),
        )
        .await
        .expect("framework result")
        .expect("get node");
    assert_eq!(queried.availability.as_str(), "offline");
    let list_handler = ListNodesHandler::new(nodes, Duration::seconds(20)).expect("list");
    let listed = list_handler
        .execute(
            ListNodes {
                organization_id: organization.id,
                queried_at: now + Duration::seconds(30),
            },
            context(),
        )
        .await
        .expect("framework result")
        .expect("list nodes");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].node.id, node_id);
}

#[test]
fn enrollment_request_debug_output_redacts_credentials_and_csr() {
    let request = NodeEnrollmentRequest {
        schema: NodeEnrollmentRequest::SCHEMA.into(),
        enrollment_token: format!("a3sn_{}", "b".repeat(64)),
        node_name: "worker".into(),
        agent_instance_id: Uuid::now_v7(),
        agent_version: "0.1.0".into(),
        csr_pem: "-----BEGIN CERTIFICATE REQUEST-----\nsecret\n-----END CERTIFICATE REQUEST-----\n"
            .into(),
        runtime_capabilities: capabilities(),
    };
    let debug = format!("{request:?}");
    assert!(!debug.contains(&request.enrollment_token));
    assert!(!debug.contains("secret"));
    assert!(debug.contains("[REDACTED]"));
}

#[test]
fn enrollment_response_schema_name_is_stable() {
    assert_eq!(
        NodeEnrollmentResponse::SCHEMA,
        "a3s.cloud.node-enrollment-response.v1"
    );
}
