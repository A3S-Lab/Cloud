use super::super::api::PeerCertificate;
use super::*;
use crate::modules::agents::infrastructure::InMemoryAgentRepository;
use crate::modules::artifacts::NodeArtifactObjectStore;
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::edge::{EdgeGatewayAcknowledgementProjector, LocalGatewayCertificateAuthority};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeProtocolSessionRepository, INodeRepository,
};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::fleet::infrastructure::{LocalCertificateAuthority, LogChunkObjectStore};
use crate::modules::inference::InMemoryInferenceUsageRepository;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_cloud_contracts::{
    InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageReceiptV1, InferenceUsageRecordV1,
};
use a3s_cloud_node_agent::FileNodeIdentityStore;
use base64::Engine;
use chrono::Duration;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn authenticated_node_accepts_usage_batch_and_holds_wrong_after() {
    let directory = tempfile::tempdir().expect("node-control directory");
    let authority = Arc::new(
        LocalCertificateAuthority::load_or_create(directory.path().join("node-ca"))
            .expect("local CA"),
    );
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let agents = Arc::new(InMemoryAgentRepository::new());
    let identity_store = FileNodeIdentityStore::new(directory.path().join("node-identity"));
    let (organization_id, enrolled_identity) =
        enroll_node(Arc::clone(&nodes), Arc::clone(&authority), &identity_store).await;
    let node_id = enrolled_identity.response.node_id;
    let usage = Arc::new(InMemoryInferenceUsageRepository::new());

    let commands: Arc<dyn INodeControlRepository> = nodes.clone();
    let sessions: Arc<dyn INodeProtocolSessionRepository> = nodes.clone();
    let node_repository: Arc<dyn INodeRepository> = nodes.clone();
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let api = NodeControlApi::new(
        node_repository,
        commands,
        sessions,
        agents,
        usage,
        Arc::new(
            NodeArtifactObjectStore::local(directory.path().join("artifacts"), 1024 * 1024)
                .expect("artifact store"),
        ),
        Arc::new(EdgeGatewayAcknowledgementProjector::new(edge.clone())),
        edge,
        Arc::new(
            LocalGatewayCertificateAuthority::load_or_create(directory.path().join("gateway-ca"))
                .expect("Gateway CA"),
        ),
        Arc::new(LogChunkObjectStore::local(directory.path()).expect("log object store")),
        authority,
        test_secret_material_handler(directory.path()),
        Duration::days(30),
        Duration::hours(1),
        Duration::minutes(5),
        Duration::minutes(5),
        Duration::seconds(30),
        StdDuration::from_millis(100),
        StdDuration::from_millis(5),
        1024 * 1024,
        StdDuration::from_secs(1),
        StdDuration::from_secs(5),
    )
    .expect("node-control API");

    let certificate = nodes
        .find_active_certificate(organization_id, NodeId::from_uuid(node_id))
        .await
        .expect("active node certificate");
    let router = api.router().layer(axum::Extension(PeerCertificate {
        fingerprint: certificate.fingerprint,
    }));

    let gateway_id = Uuid::from_u128(42);
    let epoch = Uuid::from_u128(7);
    let tip = InferenceUsageCursorV1 {
        boot_epoch: epoch,
        sequence: 1,
    };
    let first = InferenceUsageBatchV1 {
        schema: InferenceUsageBatchV1::SCHEMA.into(),
        gateway_id,
        batch_id: Uuid::from_u128(3),
        after: None,
        records: vec![record(tip, Uuid::from_u128(10))],
    };
    let response = post_usage(&router, &first).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let receipt = decode_receipt(response).await;
    receipt.validate_for(&first).expect("valid first receipt");
    assert_eq!(receipt.acknowledged_through, Some(tip));
    assert!(receipt.gaps.is_empty());

    let wrong_after = InferenceUsageBatchV1 {
        schema: InferenceUsageBatchV1::SCHEMA.into(),
        gateway_id,
        batch_id: Uuid::from_u128(4),
        after: None,
        records: vec![record(tip, Uuid::from_u128(10))],
    };
    let held = post_usage(&router, &wrong_after).await;
    assert_eq!(held.status(), axum::http::StatusCode::OK);
    let held = decode_receipt(held).await;
    held.validate_for(&wrong_after)
        .expect("valid wrong-after hold receipt");
    assert_eq!(held.acknowledged_through, Some(tip));
    assert!(held.gaps.is_empty());
}

#[tokio::test]
async fn unauthenticated_peer_cannot_post_usage_batches() {
    let directory = tempfile::tempdir().expect("node-control directory");
    let authority = Arc::new(
        LocalCertificateAuthority::load_or_create(directory.path().join("node-ca"))
            .expect("local CA"),
    );
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let agents = Arc::new(InMemoryAgentRepository::new());
    let identity_store = FileNodeIdentityStore::new(directory.path().join("node-identity"));
    let (_organization_id, _) =
        enroll_node(Arc::clone(&nodes), Arc::clone(&authority), &identity_store).await;

    let commands: Arc<dyn INodeControlRepository> = nodes.clone();
    let sessions: Arc<dyn INodeProtocolSessionRepository> = nodes.clone();
    let node_repository: Arc<dyn INodeRepository> = nodes.clone();
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let api = NodeControlApi::new(
        node_repository,
        commands,
        sessions,
        agents,
        Arc::new(InMemoryInferenceUsageRepository::new()),
        Arc::new(
            NodeArtifactObjectStore::local(directory.path().join("artifacts"), 1024 * 1024)
                .expect("artifact store"),
        ),
        Arc::new(EdgeGatewayAcknowledgementProjector::new(edge.clone())),
        edge,
        Arc::new(
            LocalGatewayCertificateAuthority::load_or_create(directory.path().join("gateway-ca"))
                .expect("Gateway CA"),
        ),
        Arc::new(LogChunkObjectStore::local(directory.path()).expect("log object store")),
        authority,
        test_secret_material_handler(directory.path()),
        Duration::days(30),
        Duration::hours(1),
        Duration::minutes(5),
        Duration::minutes(5),
        Duration::seconds(30),
        StdDuration::from_millis(100),
        StdDuration::from_millis(5),
        1024 * 1024,
        StdDuration::from_secs(1),
        StdDuration::from_secs(5),
    )
    .expect("node-control API");

    let router = api.router().layer(axum::Extension(PeerCertificate {
        fingerprint: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .into(),
    }));
    let batch = InferenceUsageBatchV1 {
        schema: InferenceUsageBatchV1::SCHEMA.into(),
        gateway_id: Uuid::from_u128(1),
        batch_id: Uuid::from_u128(2),
        after: None,
        records: vec![record(
            InferenceUsageCursorV1 {
                boot_epoch: Uuid::from_u128(3),
                sequence: 1,
            },
            Uuid::from_u128(4),
        )],
    };
    let response = post_usage(&router, &batch).await;
    assert_ne!(response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn enrolled_node_mtls_posts_usage_batches_over_live_node_control_https() {
    let directory = tempfile::tempdir().expect("usage mtls directory");
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
    let agents = Arc::new(InMemoryAgentRepository::new());
    let identity_store = FileNodeIdentityStore::new(directory.path().join("node-identity"));
    let (_, enrolled_identity) =
        enroll_node(Arc::clone(&nodes), Arc::clone(&authority), &identity_store).await;
    let commands: Arc<dyn INodeControlRepository> = nodes.clone();
    let sessions: Arc<dyn INodeProtocolSessionRepository> = nodes.clone();
    let node_repository: Arc<dyn INodeRepository> = nodes.clone();
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let api = NodeControlApi::new(
        node_repository,
        commands,
        sessions,
        agents,
        Arc::new(InMemoryInferenceUsageRepository::new()),
        Arc::new(
            NodeArtifactObjectStore::local(directory.path().join("artifacts"), 1024 * 1024)
                .expect("artifact store"),
        ),
        Arc::new(EdgeGatewayAcknowledgementProjector::new(edge.clone())),
        edge,
        Arc::new(
            LocalGatewayCertificateAuthority::load_or_create(directory.path().join("gateway-ca"))
                .expect("Gateway CA"),
        ),
        Arc::new(LogChunkObjectStore::local(directory.path()).expect("log object store")),
        authority,
        test_secret_material_handler(directory.path()),
        Duration::days(30),
        Duration::hours(1),
        Duration::minutes(5),
        Duration::minutes(5),
        Duration::seconds(30),
        StdDuration::from_millis(100),
        StdDuration::from_millis(5),
        1024 * 1024,
        StdDuration::from_secs(1),
        StdDuration::from_secs(5),
    )
    .expect("node-control API");

    let (address, listener) = bound_node_control_listener().await;
    let server = NodeControlServer::from_config(
        &NodeControlConfig {
            host: address.ip().to_string(),
            port: address.port(),
            server_name: "localhost".into(),
            certificate_file: certificate_path.to_string_lossy().into_owned(),
            private_key_file: key_path.to_string_lossy().into_owned(),
            client_ca_file: bundle_path.to_string_lossy().into_owned(),
            max_request_bytes: 1024 * 1024,
            tls_handshake_timeout_ms: 1_000,
            request_body_timeout_ms: 1_000,
        },
        api,
    )
    .expect("node-control server");
    let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
    let server_task = tokio::spawn(server.run_with_listener(listener, shutdown_receiver));

    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(
            reqwest::Certificate::from_pem(
                &std::fs::read(&bundle_path).expect("node-control CA PEM"),
            )
            .expect("node-control root"),
        )
        .identity(
            reqwest::Identity::from_pem(enrolled_identity.identity_pem().as_bytes())
                .expect("enrolled node identity"),
        )
        .build()
        .expect("mTLS client");

    let gateway_id = Uuid::from_u128(99);
    let epoch = Uuid::from_u128(11);
    let tip = InferenceUsageCursorV1 {
        boot_epoch: epoch,
        sequence: 1,
    };
    let second = InferenceUsageCursorV1 {
        boot_epoch: epoch,
        sequence: 2,
    };
    let first = InferenceUsageBatchV1 {
        schema: InferenceUsageBatchV1::SCHEMA.into(),
        gateway_id,
        batch_id: Uuid::from_u128(21),
        after: None,
        records: vec![record(tip, Uuid::from_u128(31))],
    };
    let endpoint = format!(
        "https://localhost:{}/v1/inference-control/usage-batches",
        address.port()
    );
    let response = client
        .post(&endpoint)
        .json(&first)
        .send()
        .await
        .expect("usage batch over live mTLS");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let receipt = response
        .json::<InferenceUsageReceiptV1>()
        .await
        .expect("usage receipt");
    receipt
        .validate_for(&first)
        .expect("live receipt matches batch");
    assert_eq!(receipt.acknowledged_through, Some(tip));

    let continuation = InferenceUsageBatchV1 {
        schema: InferenceUsageBatchV1::SCHEMA.into(),
        gateway_id,
        batch_id: Uuid::from_u128(22),
        after: Some(tip),
        records: vec![record(second, Uuid::from_u128(32))],
    };
    let continued = client
        .post(&endpoint)
        .json(&continuation)
        .send()
        .await
        .expect("continuation batch")
        .json::<InferenceUsageReceiptV1>()
        .await
        .expect("continuation receipt");
    continued
        .validate_for(&continuation)
        .expect("continuation receipt");
    assert_eq!(continued.acknowledged_through, Some(second));

    // Stale `after` while the durable tip is still carried in this batch:
    // Cloud holds the true watermark without inventing contiguity.
    let wrong_after = InferenceUsageBatchV1 {
        schema: InferenceUsageBatchV1::SCHEMA.into(),
        gateway_id,
        batch_id: Uuid::from_u128(23),
        after: Some(tip),
        records: vec![record(second, Uuid::from_u128(32))],
    };
    let held = client
        .post(&endpoint)
        .json(&wrong_after)
        .send()
        .await
        .expect("wrong-after batch");
    assert_eq!(held.status(), reqwest::StatusCode::OK);
    let held = held
        .json::<InferenceUsageReceiptV1>()
        .await
        .expect("wrong-after receipt");
    held.validate_for(&wrong_after).expect("hold receipt");
    assert_eq!(held.acknowledged_through, Some(second));
    assert!(held.gaps.is_empty());

    let _ = shutdown_sender.send(true);
    let _ = server_task.await;
}

fn lifecycle_payload() -> Vec<u8> {
    use a3s_cloud_contracts::{
        InferenceUsageEndpointV1, InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
        InferenceUsageRequestEvidenceV1,
    };
    use chrono::{DateTime, Utc};
    let event = InferenceUsageLifecycleEventV1 {
        schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
        kind: InferenceUsageLifecycleKindV1::RequestStarted,
        occurred_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        request: InferenceUsageRequestEvidenceV1 {
            request_id: Uuid::from_u128(100),
            correlation_id: "corr".into(),
            environment_id: Uuid::from_u128(101),
            credential_id: Uuid::from_u128(102),
            credential_generation: 1,
            route_id: Uuid::from_u128(103),
            route_policy_revision: 1,
            endpoint: InferenceUsageEndpointV1::ChatCompletions,
            model_alias: "alias".into(),
            model_id: Uuid::from_u128(104),
        },
        attempt: None,
        outcome: None,
        http_status: None,
        duration_ms: None,
        measurement_completeness: None,
        total_tokens: None,
    };
    serde_json::to_vec(&event).unwrap()
}

fn record(cursor: InferenceUsageCursorV1, event_id: Uuid) -> InferenceUsageRecordV1 {
    let payload = lifecycle_payload();
    InferenceUsageRecordV1 {
        cursor,
        event_id,
        payload_base64: base64::engine::general_purpose::STANDARD.encode(&payload),
        payload_sha256: format!("{:x}", Sha256::digest(&payload)),
    }
}

async fn post_usage(
    router: &axum::Router,
    batch: &InferenceUsageBatchV1,
) -> axum::http::Response<axum::body::Body> {
    router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/inference-control/usage-batches")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(batch).expect("encode usage batch"),
                ))
                .expect("build usage request"),
        )
        .await
        .expect("usage route")
}

async fn decode_receipt(
    response: axum::http::Response<axum::body::Body>,
) -> InferenceUsageReceiptV1 {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("usage receipt body");
    serde_json::from_slice(&bytes).expect("decode usage receipt")
}
