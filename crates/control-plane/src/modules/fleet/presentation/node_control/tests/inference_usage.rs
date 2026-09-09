use super::super::api::PeerCertificate;
use super::{enroll_node, test_secret_material_handler, NodeControlApi};
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
use chrono::{Duration, Utc};
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
        records: vec![record(tip, Uuid::from_u128(10), b"a")],
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
        records: vec![record(tip, Uuid::from_u128(10), b"a")],
    };
    let held = post_usage(&router, &wrong_after).await;
    assert_eq!(held.status(), axum::http::StatusCode::OK);
    let held = decode_receipt(held).await;
    held.validate_for(&wrong_after)
        .expect("valid wrong-after hold receipt");
    assert_eq!(held.acknowledged_through, Some(tip));
    assert!(held.gaps.is_empty());

    let _ = organization_id;
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
            b"x",
        )],
    };
    let response = post_usage(&router, &batch).await;
    assert_ne!(response.status(), axum::http::StatusCode::OK);
}

fn record(cursor: InferenceUsageCursorV1, event_id: Uuid, payload: &[u8]) -> InferenceUsageRecordV1 {
    InferenceUsageRecordV1 {
        cursor,
        event_id,
        payload_base64: base64::engine::general_purpose::STANDARD.encode(payload),
        payload_sha256: format!("{:x}", Sha256::digest(payload)),
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

async fn decode_receipt(response: axum::http::Response<axum::body::Body>) -> InferenceUsageReceiptV1 {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("usage receipt body");
    serde_json::from_slice(&bytes).expect("decode usage receipt")
}
