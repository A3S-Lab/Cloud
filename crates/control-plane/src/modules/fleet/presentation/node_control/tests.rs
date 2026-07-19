use super::{NodeControlApi, NodeControlServer};
use crate::config::NodeControlConfig;
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::edge::EdgeGatewayAcknowledgementProjector;
use crate::modules::fleet::application::{EnrollNode, EnrollNodeHandler};
use crate::modules::fleet::domain::entities::{EnrollmentToken, NodeCommandDraft};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeStateChange,
};
use crate::modules::fleet::domain::services::{ICertificateAuthority, NodeCertificateRequest};
use crate::modules::fleet::domain::value_objects::{EnrollmentTokenCredential, NodeState};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::fleet::infrastructure::{LocalCertificateAuthority, LocalLogChunkStore};
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, IdempotencyRequest, NodeCertificateId, NodeCommandId, NodeId, OrganizationId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    DomainEventEnvelope, GatewayAckState, GatewaySnapshot, NodeCertificateRotationRequest,
    NodeCertificateRotationResponse, NodeCommandAck, NodeCommandAckReceipt,
    NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult, NodeGatewayAck, NodeGatewayAckReceipt, NodeHeartbeat, NodeLogChunkBatch,
    NodeLogChunkReceipt, NodeLogChunkReport, NodeObservationBatch, NodeObservationReceipt,
    NodeProtocolError, NodeProtocolErrorCode, RuntimeObservationReport,
};
use a3s_cloud_node_agent::{EnrolledNodeIdentity, FileNodeIdentityStore, NodeIdentityState};
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeFeature,
    RuntimeInspection, RuntimeLogChunk, RuntimeLogStream, RuntimeObservation, RuntimeUnitClass,
    RuntimeUnitState,
};
use chrono::{Duration, Utc};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use sha2::{Digest, Sha256};
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn node_control_requires_real_mtls_and_authenticates_the_peer_leaf() {
    let directory = tempfile::tempdir().expect("node-control directory");
    let authority = Arc::new(
        LocalCertificateAuthority::load_or_create(directory.path().join("node-ca"))
            .expect("local CA"),
    );
    let certificate_path = directory.path().join("server.pem");
    let key_path = directory.path().join("server-key.pem");
    let bundle_path = directory.path().join("ca.pem");
    authority
        .ensure_ca_bundle(&bundle_path)
        .expect("client CA bundle");
    authority
        .ensure_server_identity("localhost", &certificate_path, &key_path)
        .expect("server identity");

    let nodes = Arc::new(InMemoryNodeRepository::new());
    let identity_store = FileNodeIdentityStore::new(directory.path().join("node-identity"));
    let (organization_id, enrolled_identity) =
        enroll_node(Arc::clone(&nodes), Arc::clone(&authority), &identity_store).await;
    let enrollment = enrolled_identity.response.clone();
    let agent_instance_id = enrolled_identity.agent_instance_id;
    let node_id = enrollment.node_id;
    let commands: Arc<dyn INodeControlRepository> = nodes.clone();
    let node_repository: Arc<dyn INodeRepository> = nodes.clone();
    let log_store =
        Arc::new(LocalLogChunkStore::new(directory.path().join("logs")).expect("log object store"));
    let api = NodeControlApi::new(
        node_repository,
        commands,
        Arc::new(EdgeGatewayAcknowledgementProjector::new(Arc::new(
            InMemoryEdgeRepository::new(),
        ))),
        log_store,
        authority.clone(),
        Duration::hours(1),
        Duration::milliseconds(250),
        Duration::seconds(30),
        StdDuration::from_millis(100),
        StdDuration::from_millis(5),
        1024 * 1024,
        StdDuration::from_millis(50),
    )
    .expect("node-control API");
    let address = unused_address();
    let config = NodeControlConfig {
        host: address.ip().to_string(),
        port: address.port(),
        server_name: "localhost".into(),
        certificate_file: certificate_path.to_string_lossy().into_owned(),
        private_key_file: key_path.to_string_lossy().into_owned(),
        client_ca_file: bundle_path.to_string_lossy().into_owned(),
        max_request_bytes: 1024 * 1024,
        tls_handshake_timeout_ms: 1_000,
        request_body_timeout_ms: 1_000,
    };
    let server = NodeControlServer::from_config(&config, api.clone()).expect("node-control server");
    let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
    let server_task = tokio::spawn(server.run(shutdown_receiver));
    wait_until_listening(address).await;

    let ca = std::fs::read(&bundle_path).expect("CA PEM");
    let root = reqwest::Certificate::from_pem(&ca).expect("root certificate");
    let without_identity = reqwest::Client::builder()
        .add_root_certificate(root.clone())
        .build()
        .expect("client without identity");
    let endpoint = format!(
        "https://localhost:{}/v1/node-control/commands:lease",
        address.port()
    );
    assert!(without_identity
        .post(&endpoint)
        .json(&lease_request(node_id, agent_instance_id))
        .send()
        .await
        .is_err());

    let foreign_authority =
        LocalCertificateAuthority::load_or_create(directory.path().join("foreign-node-ca"))
            .expect("foreign CA");
    let (foreign_key, foreign_csr) = certificate_request("foreign-node");
    let issued_at = Utc::now();
    let foreign_certificate = foreign_authority
        .issue(NodeCertificateRequest {
            certificate_id: NodeCertificateId::new(),
            node_id: NodeId::new(),
            csr_pem: foreign_csr,
            issued_at,
            expires_at: issued_at + Duration::hours(1),
        })
        .await
        .expect("foreign client certificate");
    let foreign_identity = format!(
        "{}\n{}",
        foreign_certificate.certificate_pem,
        foreign_key.serialize_pem()
    );
    let foreign_client = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&ca).expect("server root for foreign client"),
        )
        .identity(
            reqwest::Identity::from_pem(foreign_identity.as_bytes()).expect("foreign identity"),
        )
        .build()
        .expect("foreign mTLS client");
    assert!(foreign_client
        .post(&endpoint)
        .json(&lease_request(node_id, agent_instance_id))
        .send()
        .await
        .is_err());

    let identity_pem = enrolled_identity.identity_pem();
    let client = reqwest::Client::builder()
        .add_root_certificate(root)
        .identity(reqwest::Identity::from_pem(identity_pem.as_bytes()).expect("client identity"))
        .build()
        .expect("mTLS client");
    let response = client
        .post(&endpoint)
        .json(&lease_request(node_id, agent_instance_id))
        .send()
        .await
        .expect("lease request");
    let status = response.status();
    let body = response.bytes().await.expect("lease response body");
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let lease: NodeCommandLeaseResponse = serde_json::from_slice(&body).expect("lease response");
    assert_eq!(lease.node_id, node_id);
    assert!(lease.commands.is_empty());
    lease.validate(Utc::now()).expect("valid lease response");

    let issued_at = Utc::now();
    nodes
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id: NodeId::from_uuid(node_id),
            aggregate_id: Uuid::now_v7(),
            payload: NodeCommandPayload::RuntimeInspect {
                unit_id: "service-1".into(),
                generation: 1,
            },
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        })
        .await
        .expect("enqueue command");
    let leased = client
        .post(&endpoint)
        .json(&lease_request(node_id, agent_instance_id))
        .send()
        .await
        .expect("lease command")
        .json::<NodeCommandLeaseResponse>()
        .await
        .expect("leased command response");
    assert_eq!(leased.commands.len(), 1);
    let command = &leased.commands[0];
    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: Utc::now(),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeInspected {
                inspection: RuntimeInspection::NotFound {
                    schema: RuntimeInspection::SCHEMA.into(),
                    unit_id: "service-1".into(),
                    last_generation: Some(1),
                },
            }),
        },
    };
    let acknowledgement_endpoint = format!(
        "https://localhost:{}/v1/node-control/commands/{}:ack",
        address.port(),
        command.command_id
    );
    let first_ack = client
        .post(&acknowledgement_endpoint)
        .json(&acknowledgement)
        .send()
        .await
        .expect("acknowledge command");
    assert_eq!(first_ack.status(), reqwest::StatusCode::OK);
    assert!(
        !first_ack
            .json::<NodeCommandAckReceipt>()
            .await
            .expect("acknowledgement receipt")
            .replayed
    );
    let replayed_ack = client
        .post(&acknowledgement_endpoint)
        .json(&acknowledgement)
        .send()
        .await
        .expect("replay acknowledgement")
        .json::<NodeCommandAckReceipt>()
        .await
        .expect("replayed acknowledgement receipt");
    assert!(replayed_ack.replayed);

    let observed_at = Utc::now();
    let observations = NodeObservationBatch {
        schema: NodeObservationBatch::SCHEMA.into(),
        node_id,
        agent_instance_id,
        sent_at: observed_at,
        heartbeat: NodeHeartbeat {
            schema: NodeHeartbeat::SCHEMA.into(),
            node_id,
            agent_instance_id,
            observed_at,
            agent_version: "0.1.0".into(),
            runtime_capabilities: capabilities(),
        },
        observations: vec![RuntimeObservationReport {
            report_id: Uuid::now_v7(),
            command_id: None,
            observed_at,
            observation: runtime_observation("service-1", 1, 1),
        }],
    };
    let observations_endpoint = format!(
        "https://localhost:{}/v1/node-control/observations",
        address.port()
    );
    let first_observation = client
        .post(&observations_endpoint)
        .json(&observations)
        .send()
        .await
        .expect("record observations")
        .json::<NodeObservationReceipt>()
        .await
        .expect("observation receipt");
    assert_eq!(first_observation.accepted_reports, 1);
    assert_eq!(first_observation.replayed_reports, 0);
    let replayed_observation = client
        .post(&observations_endpoint)
        .json(&observations)
        .send()
        .await
        .expect("replay observations")
        .json::<NodeObservationReceipt>()
        .await
        .expect("replayed observation receipt");
    assert_eq!(replayed_observation.accepted_reports, 0);
    assert_eq!(replayed_observation.replayed_reports, 1);

    let snapshot =
        GatewaySnapshot::new(1, None, "management { enabled = true }\n").expect("Gateway snapshot");
    let gateway_command = nodes
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id: NodeId::from_uuid(node_id),
            aggregate_id: Uuid::now_v7(),
            payload: NodeCommandPayload::GatewaySnapshotInstall {
                snapshot: Box::new(snapshot.clone()),
            },
            issued_at: Utc::now(),
            not_after: Utc::now() + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        })
        .await
        .expect("enqueue Gateway command")
        .value;
    let gateway_ack = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: gateway_command.id.as_uuid(),
        node_id,
        revision: snapshot.revision,
        snapshot_digest: snapshot.snapshot_digest,
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: Utc::now(),
    };
    let gateway_endpoint = format!(
        "https://localhost:{}/v1/node-control/gateway-acks",
        address.port()
    );
    let first_gateway = client
        .post(&gateway_endpoint)
        .json(&gateway_ack)
        .send()
        .await
        .expect("record Gateway acknowledgement")
        .json::<NodeGatewayAckReceipt>()
        .await
        .expect("Gateway acknowledgement receipt");
    assert!(!first_gateway.replayed);
    assert_eq!(first_gateway.command_id, gateway_command.id.as_uuid());
    let replayed_gateway = client
        .post(&gateway_endpoint)
        .json(&gateway_ack)
        .send()
        .await
        .expect("replay Gateway acknowledgement")
        .json::<NodeGatewayAckReceipt>()
        .await
        .expect("replayed Gateway acknowledgement receipt");
    assert!(replayed_gateway.replayed);

    let mut wrong_revision = gateway_ack.clone();
    wrong_revision.acknowledgement_id = Uuid::now_v7();
    wrong_revision.revision += 1;
    assert_eq!(
        client
            .post(&gateway_endpoint)
            .json(&wrong_revision)
            .send()
            .await
            .expect("reject mismatched Gateway acknowledgement")
            .status(),
        reqwest::StatusCode::CONFLICT
    );

    let log_data = "service started";
    let log_batch = NodeLogChunkBatch {
        schema: NodeLogChunkBatch::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id,
        sent_at: Utc::now(),
        chunks: vec![NodeLogChunkReport {
            unit_id: "service-1".into(),
            generation: 1,
            chunk: RuntimeLogChunk {
                schema: RuntimeLogChunk::SCHEMA.into(),
                cursor: "opaque:1".into(),
                sequence: 1,
                observed_at_ms: 1,
                stream: RuntimeLogStream::Stdout,
                data: log_data.into(),
            },
            checksum: format!("sha256:{:x}", Sha256::digest(log_data.as_bytes())),
        }],
    };
    let logs_endpoint = format!(
        "https://localhost:{}/v1/node-control/log-chunks",
        address.port()
    );
    let first_logs = client
        .post(&logs_endpoint)
        .json(&log_batch)
        .send()
        .await
        .expect("record logs")
        .json::<NodeLogChunkReceipt>()
        .await
        .expect("log receipt");
    assert!(!first_logs.replayed);
    let replayed_logs = client
        .post(&logs_endpoint)
        .json(&log_batch)
        .send()
        .await
        .expect("replay logs")
        .json::<NodeLogChunkReceipt>()
        .await
        .expect("replayed log receipt");
    assert!(replayed_logs.replayed);

    let oversized = client
        .post(&endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(vec![b' '; 1024 * 1024 + 1])
        .send()
        .await
        .expect("oversized response");
    assert_eq!(oversized.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let oversized: NodeProtocolError = oversized.json().await.expect("oversized protocol error");
    assert_eq!(oversized.code, NodeProtocolErrorCode::PayloadTooLarge);

    let active_certificate = nodes
        .find_active_certificate(organization_id, NodeId::from_uuid(node_id))
        .await
        .expect("active certificate");
    let slow_body = async_stream::stream! {
        tokio::time::sleep(StdDuration::from_millis(100)).await;
        yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"{}"));
    };
    let slow_request = axum::http::Request::builder()
        .method(axum::http::Method::POST)
        .uri("/v1/node-control/commands:lease")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from_stream(slow_body))
        .expect("slow request");
    let slow_response = api
        .router()
        .layer(axum::Extension(super::api::PeerCertificate {
            fingerprint: active_certificate.fingerprint,
        }))
        .oneshot(slow_request)
        .await
        .expect("slow body response");
    assert_eq!(
        slow_response.status(),
        axum::http::StatusCode::REQUEST_TIMEOUT
    );

    let mismatch = client
        .post(&endpoint)
        .json(&lease_request(Uuid::now_v7(), agent_instance_id))
        .send()
        .await
        .expect("identity mismatch response");
    assert_eq!(mismatch.status(), reqwest::StatusCode::FORBIDDEN);
    let mismatch: NodeProtocolError = mismatch.json().await.expect("protocol error");
    assert_eq!(mismatch.code, NodeProtocolErrorCode::Forbidden);
    mismatch.validate().expect("versioned protocol error");

    let prepared_rotation = identity_store
        .prepare_rotation()
        .await
        .expect("persist replacement key and CSR");
    let rotation_request = prepared_rotation
        .pending_rotation_request()
        .expect("pending rotation request");
    let rotation_endpoint = format!(
        "https://localhost:{}/v1/node-control/certificate:rotate",
        address.port()
    );
    let rotation = client
        .post(&rotation_endpoint)
        .json(&rotation_request)
        .send()
        .await
        .expect("rotate certificate");
    let rotation_status = rotation.status();
    let rotation_body = rotation.bytes().await.expect("rotation response body");
    assert_eq!(
        rotation_status,
        reqwest::StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&rotation_body)
    );
    let rotation: NodeCertificateRotationResponse =
        serde_json::from_slice(&rotation_body).expect("rotation response");
    rotation.validate().expect("valid rotation response");
    assert_eq!(rotation.node_id, node_id);
    assert_eq!(
        rotation.previous_certificate_id,
        enrollment.certificate.certificate_id
    );
    assert!(!rotation.replayed);

    // Simulate a process crash after the server committed the replacement but before the
    // first response was persisted. A reopened store must retain the old certificate and the
    // exact replacement private key and CSR.
    let restarted_store = FileNodeIdentityStore::new(directory.path().join("node-identity"));
    let restarted_identity = match restarted_store
        .load()
        .await
        .expect("reload node identity after simulated crash")
        .expect("persisted node identity")
    {
        NodeIdentityState::Enrolled(identity) => identity,
        NodeIdentityState::Pending(_) => panic!("enrollment must remain complete"),
    };
    assert_eq!(
        restarted_identity.pending_rotation_request(),
        Some(rotation_request.clone())
    );
    let restarted_old_client = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&ca).expect("server root after rotation crash"),
        )
        .identity(
            reqwest::Identity::from_pem(restarted_identity.identity_pem().as_bytes())
                .expect("persisted old identity"),
        )
        .build()
        .expect("restarted old mTLS client");
    let replay = restarted_old_client
        .post(&rotation_endpoint)
        .json(&rotation_request)
        .send()
        .await
        .expect("replay rotation")
        .json::<NodeCertificateRotationResponse>()
        .await
        .expect("replayed rotation response");
    assert!(replay.replayed);
    assert_eq!(replay.certificate, rotation.certificate);
    let rotated_identity = restarted_store
        .complete_rotation(replay.clone())
        .await
        .expect("atomically persist replacement identity");

    let (_, conflicting_csr) = certificate_request("node-control-conflict");
    let conflicting_rotation = restarted_old_client
        .post(&rotation_endpoint)
        .json(&NodeCertificateRotationRequest {
            csr_pem: conflicting_csr,
            ..rotation_request.clone()
        })
        .send()
        .await
        .expect("conflicting rotation response");
    assert_eq!(conflicting_rotation.status(), reqwest::StatusCode::CONFLICT);

    let old_certificate_lease = restarted_old_client
        .post(&endpoint)
        .json(&lease_request(node_id, agent_instance_id))
        .send()
        .await
        .expect("old certificate lease response");
    assert_eq!(
        old_certificate_lease.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let client = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&ca).expect("server root for replacement client"),
        )
        .identity(
            reqwest::Identity::from_pem(rotated_identity.identity_pem().as_bytes())
                .expect("replacement identity"),
        )
        .build()
        .expect("replacement mTLS client");
    let replacement_lease = client
        .post(&endpoint)
        .json(&lease_request(node_id, agent_instance_id))
        .send()
        .await
        .expect("replacement certificate lease response");
    assert_eq!(replacement_lease.status(), reqwest::StatusCode::OK);

    tokio::time::sleep(StdDuration::from_millis(275)).await;
    let expired_replay = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&ca).expect("server root for old replay client"),
        )
        .identity(
            reqwest::Identity::from_pem(identity_pem.as_bytes()).expect("old replay identity"),
        )
        .build()
        .expect("old replay client")
        .post(&rotation_endpoint)
        .json(&rotation_request)
        .send()
        .await
        .expect("expired rotation replay response");
    assert_eq!(expired_replay.status(), reqwest::StatusCode::UNAUTHORIZED);

    let node = nodes
        .find(
            organization_id,
            crate::modules::shared_kernel::domain::NodeId::from_uuid(node_id),
        )
        .await
        .expect("enrolled node");
    nodes
        .set_state(NodeStateChange {
            organization_id,
            node_id: node.id,
            state: NodeState::Revoked,
            expected_version: node.aggregate_version,
            changed_at: Utc::now(),
            event: event(
                organization_id,
                node_id,
                node.aggregate_version + 1,
                "fleet.node.revoked",
            ),
            idempotency: IdempotencyRequest::new("node-state", "revoke", node_id.as_bytes())
                .expect("state idempotency"),
        })
        .await
        .expect("revoke node");
    let revoked = client
        .post(&endpoint)
        .json(&lease_request(node_id, agent_instance_id))
        .send()
        .await
        .expect("revoked certificate response");
    assert_eq!(revoked.status(), reqwest::StatusCode::UNAUTHORIZED);
    let revoked: NodeProtocolError = revoked.json().await.expect("revoked protocol error");
    assert_eq!(revoked.code, NodeProtocolErrorCode::Unauthenticated);

    shutdown_sender.send(true).expect("request shutdown");
    server_task
        .await
        .expect("node-control task")
        .expect("node-control shutdown");
}

async fn enroll_node(
    nodes: Arc<InMemoryNodeRepository>,
    authority: Arc<LocalCertificateAuthority>,
    identity_store: &FileNodeIdentityStore,
) -> (OrganizationId, EnrolledNodeIdentity) {
    let organization_id = OrganizationId::new();
    let now = Utc::now();
    let token_secret = format!("a3sn_{}", "d".repeat(64));
    let credential = EnrollmentTokenCredential::from_secret(&token_secret).expect("credential");
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "node-control test",
        credential,
        now,
        now + Duration::minutes(10),
    )
    .expect("enrollment token");
    nodes
        .issue_enrollment_token(
            token.clone(),
            event(
                organization_id,
                token.id.as_uuid(),
                token.aggregate_version,
                "fleet.enrollment-token.issued",
            ),
            IdempotencyRequest::new("enrollment", "node-control", b"node-control")
                .expect("token idempotency"),
        )
        .await
        .expect("issue enrollment token");
    let pending = match identity_store
        .prepare("node-control-test".into(), "0.1.0".into(), capabilities())
        .await
        .expect("prepare node identity")
    {
        NodeIdentityState::Pending(pending) => pending,
        NodeIdentityState::Enrolled(_) => panic!("new node identity must be pending"),
    };
    let request = pending.enrollment_request(token_secret);
    let handler = EnrollNodeHandler::new(
        nodes,
        authority,
        Duration::hours(1),
        15 * 60 * 1_000,
        5_000,
        100,
    )
    .expect("enrollment handler");
    let result = handler
        .execute(
            EnrollNode {
                request,
                request_id: Uuid::now_v7(),
                received_at: now,
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("framework enrollment")
        .expect("node enrollment");
    let identity = identity_store
        .complete(result.response)
        .await
        .expect("persist enrolled node identity");
    (organization_id, identity)
}

fn certificate_request(common_name: &str) -> (KeyPair, String) {
    let key = KeyPair::generate().expect("client private key");
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    params.distinguished_name = name;
    let csr = params
        .serialize_request(&key)
        .expect("client CSR")
        .pem()
        .expect("client CSR PEM");
    (key, csr)
}

fn lease_request(node_id: Uuid, agent_instance_id: Uuid) -> NodeCommandLeaseRequest {
    NodeCommandLeaseRequest {
        schema: NodeCommandLeaseRequest::SCHEMA.into(),
        node_id,
        agent_instance_id,
        after_sequence: 0,
        max_commands: 1,
        wait_ms: 0,
    }
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
        resource_controls: vec![ResourceControl::Cpu, ResourceControl::Memory],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
        ],
    }
}

fn runtime_observation(unit_id: &str, generation: u64, observed_at_ms: u64) -> RuntimeObservation {
    RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: unit_id.into(),
        generation,
        spec_digest: format!("sha256:{}", "7".repeat(64)),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Accepted,
        provider_resource_id: None,
        provider_build: None,
        observed_at_ms,
        started_at_ms: None,
        finished_at_ms: None,
        health: None,
        outputs: Vec::new(),
        usage: None,
        evidence: None,
        provider_attestation: None,
        failure: None,
    }
}

fn event(
    organization_id: OrganizationId,
    aggregate_id: Uuid,
    aggregate_version: u64,
    event_key: &str,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id,
        aggregate_version,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}

fn unused_address() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("ephemeral port");
    listener.local_addr().expect("local address")
}

async fn wait_until_listening(address: SocketAddr) {
    let deadline = tokio::time::Instant::now() + StdDuration::from_secs(2);
    loop {
        if tokio::net::TcpStream::connect(address).await.is_ok() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "node-control listener did not start"
        );
        tokio::time::sleep(StdDuration::from_millis(10)).await;
    }
}
