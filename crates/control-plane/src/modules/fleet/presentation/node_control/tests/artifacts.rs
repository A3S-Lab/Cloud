use super::*;
use crate::modules::artifacts::{
    INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactStoreError,
};
use a3s_cloud_contracts::{
    artifact_uri, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{
    ArtifactRef, ResourceLimits, RestartPolicy, RuntimeApplyRequest, RuntimeMount,
    RuntimeMountSource, RuntimeNetworkSpec, RuntimeOutputSpec, RuntimeProcessSpec, RuntimeUnitSpec,
};
use std::collections::BTreeMap;
use std::io::Cursor;

#[tokio::test]
async fn node_artifact_transport_streams_exact_bytes_and_enforces_command_authority() {
    let directory = tempfile::tempdir().expect("node artifact transport directory");
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
    let (_, enrolled_identity) =
        enroll_node(Arc::clone(&nodes), Arc::clone(&authority), &identity_store).await;
    let node_id = enrolled_identity.response.node_id;
    let artifact_store = Arc::new(
        LocalNodeArtifactStore::new(directory.path().join("artifacts"), 1024 * 1024)
            .expect("artifact store"),
    );
    let artifact_binding: Arc<dyn INodeArtifactStore> = artifact_store.clone();
    let commands: Arc<dyn INodeControlRepository> = nodes.clone();
    let node_repository: Arc<dyn INodeRepository> = nodes.clone();
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let api = NodeControlApi::new(
        node_repository,
        commands,
        artifact_binding,
        Arc::new(EdgeGatewayAcknowledgementProjector::new(edge.clone())),
        edge,
        Arc::new(
            LocalGatewayCertificateAuthority::load_or_create(directory.path().join("gateway-ca"))
                .expect("Gateway CA"),
        ),
        Arc::new(LocalLogChunkStore::new(directory.path().join("logs")).expect("log object store")),
        authority,
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(
            crate::modules::fleet::infrastructure::LocalKeyEncryptionService::load_or_create(
                directory.path().join("secret-key"),
            )
            .expect("Secret encryption"),
        ),
        Duration::days(30),
        Duration::hours(1),
        Duration::minutes(5),
        Duration::seconds(30),
        StdDuration::from_millis(100),
        StdDuration::from_millis(5),
        1024 * 1024,
        StdDuration::from_secs(1),
        StdDuration::from_secs(5),
    )
    .expect("node-control API");
    let address = unused_address();
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
    let server_task = tokio::spawn(server.run(shutdown_receiver));
    wait_until_listening(address).await;
    let client = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(
                &std::fs::read(&bundle_path).expect("node-control CA PEM"),
            )
            .expect("node-control root"),
        )
        .identity(
            reqwest::Identity::from_pem(enrolled_identity.identity_pem().as_bytes())
                .expect("node identity"),
        )
        .build()
        .expect("mTLS client");

    let input_bytes = b"content-addressed source archive";
    let input = cloud_artifact(input_bytes);
    artifact_store
        .put(
            &NodeArtifactDescriptor::new(input.clone(), input_bytes.len() as u64)
                .expect("input descriptor"),
            Box::pin(Cursor::new(input_bytes.to_vec())),
        )
        .await
        .expect("preload input artifact");
    let spec = artifact_task_spec(input.clone());
    let command = enqueue_apply(
        &nodes,
        node_id,
        spec.clone(),
        Utc::now() + Duration::minutes(1),
    )
    .await;
    let spec_digest = spec.digest().expect("Runtime spec digest");
    let download = NodeArtifactDownloadRequest::new(
        node_id,
        command.id.as_uuid(),
        spec_digest.clone(),
        "source",
        &input,
    )
    .expect("download request");
    let download_endpoint = format!(
        "https://localhost:{}/v1/node-control/artifacts:download",
        address.port()
    );
    let response = client
        .get(&download_endpoint)
        .query(&download)
        .send()
        .await
        .expect("download artifact");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .expect("download cache policy"),
        "no-store"
    );
    assert_eq!(
        response
            .headers()
            .get("x-a3s-artifact-digest")
            .expect("download Artifact digest"),
        input.digest.as_str()
    );
    assert_eq!(
        response.bytes().await.expect("download bytes").as_ref(),
        input_bytes
    );

    let mut forged_node = download.clone();
    forged_node.node_id = Uuid::now_v7();
    assert_download_status(
        &client,
        &download_endpoint,
        &forged_node,
        reqwest::StatusCode::FORBIDDEN,
    )
    .await;
    let mut forged_command = download.clone();
    forged_command.command_id = Uuid::now_v7();
    assert_download_status(
        &client,
        &download_endpoint,
        &forged_command,
        reqwest::StatusCode::FORBIDDEN,
    )
    .await;
    let mut forged_spec = download.clone();
    forged_spec.spec_digest = format!("sha256:{}", "f".repeat(64));
    assert_download_status(
        &client,
        &download_endpoint,
        &forged_spec,
        reqwest::StatusCode::FORBIDDEN,
    )
    .await;
    let mut forged_mount = download.clone();
    forged_mount.mount_name = "other".into();
    assert_download_status(
        &client,
        &download_endpoint,
        &forged_mount,
        reqwest::StatusCode::FORBIDDEN,
    )
    .await;
    let mut forged_media_type = download.clone();
    forged_media_type.artifact_media_type = "application/octet-stream".into();
    assert_download_status(
        &client,
        &download_endpoint,
        &forged_media_type,
        reqwest::StatusCode::BAD_REQUEST,
    )
    .await;

    let output_bytes = b"validated OCI layout archive";
    let output_digest = format!("sha256:{:x}", Sha256::digest(output_bytes));
    let upload = NodeArtifactUploadRequest::new(
        node_id,
        command.id.as_uuid(),
        spec_digest,
        "oci-layout",
        output_digest.clone(),
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        output_bytes.len() as u64,
    )
    .expect("upload request");
    let upload_endpoint = format!(
        "https://localhost:{}/v1/node-control/artifacts:upload",
        address.port()
    );
    let first = send_upload(&client, &upload_endpoint, &upload, output_bytes).await;
    assert_eq!(first.0, reqwest::StatusCode::CREATED);
    assert!(!first.1.replayed);
    first.1.validate_against(&upload).expect("upload receipt");
    let replay = send_upload(&client, &upload_endpoint, &upload, output_bytes).await;
    assert_eq!(replay.0, reqwest::StatusCode::OK);
    assert!(replay.1.replayed);
    assert_eq!(replay.1.artifact, first.1.artifact);

    let mut wrong_digest = upload.clone();
    wrong_digest.digest = format!("sha256:{}", "0".repeat(64));
    assert_upload_status(
        &client,
        &upload_endpoint,
        &wrong_digest,
        output_bytes,
        reqwest::StatusCode::BAD_REQUEST,
    )
    .await;
    let mut wrong_size = upload.clone();
    wrong_size.size_bytes += 1;
    assert_upload_status(
        &client,
        &upload_endpoint,
        &wrong_size,
        output_bytes,
        reqwest::StatusCode::BAD_REQUEST,
    )
    .await;
    let mut wrong_output = upload.clone();
    wrong_output.output_name = "other".into();
    assert_upload_status(
        &client,
        &upload_endpoint,
        &wrong_output,
        output_bytes,
        reqwest::StatusCode::FORBIDDEN,
    )
    .await;

    let expired_spec_digest = spec.digest().expect("expired spec digest");
    let expires_at = Utc::now() + Duration::milliseconds(200);
    let expired = enqueue_apply(&nodes, node_id, spec, expires_at).await;
    tokio::time::sleep(StdDuration::from_millis(250)).await;
    let expired_download = NodeArtifactDownloadRequest::new(
        node_id,
        expired.id.as_uuid(),
        expired_spec_digest,
        "source",
        &input,
    )
    .expect("expired download request");
    assert_download_status(
        &client,
        &download_endpoint,
        &expired_download,
        reqwest::StatusCode::FORBIDDEN,
    )
    .await;

    let published = cloud_artifact(output_bytes);
    let mut opened = artifact_store
        .open(&published)
        .await
        .expect("replay published output");
    let mut replayed_bytes = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut opened.reader, &mut replayed_bytes)
        .await
        .expect("read published output");
    assert_eq!(replayed_bytes, output_bytes);
    assert!(matches!(
        artifact_store
            .open(&ArtifactRef {
                uri: artifact_uri(&wrong_digest.digest).expect("forged URI"),
                digest: wrong_digest.digest,
                media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            })
            .await,
        Err(NodeArtifactStoreError::NotFound)
    ));

    shutdown_sender.send(true).expect("request shutdown");
    server_task
        .await
        .expect("node-control task")
        .expect("node-control shutdown");
}

async fn enqueue_apply(
    nodes: &Arc<InMemoryNodeRepository>,
    node_id: Uuid,
    spec: RuntimeUnitSpec,
    not_after: chrono::DateTime<Utc>,
) -> crate::modules::fleet::domain::entities::NodeCommand {
    let issued_at = Utc::now();
    nodes
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id: NodeId::from_uuid(node_id),
            aggregate_id: Uuid::now_v7(),
            payload: NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("artifact-apply-{}", Uuid::now_v7()),
                    deadline_at_ms: None,
                    spec,
                }),
            },
            issued_at,
            not_after,
            correlation_id: Uuid::now_v7(),
        })
        .await
        .expect("enqueue artifact apply command")
        .value
}

fn artifact_task_spec(input: ArtifactRef) -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: format!("artifact-task-{}", Uuid::now_v7()),
        generation: 1,
        class: RuntimeUnitClass::Task,
        artifact: ArtifactRef {
            uri: format!("oci://registry.example/build@sha256:{}", "a".repeat(64)),
            digest: format!("sha256:{}", "a".repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: RuntimeProcessSpec {
            command: vec!["/bin/build".into()],
            args: Vec::new(),
            working_directory: Some("/workspace".into()),
            environment: BTreeMap::new(),
        },
        mounts: vec![RuntimeMount {
            name: "source".into(),
            source: RuntimeMountSource::Artifact { artifact: input },
            target: "/workspace".into(),
            read_only: true,
        }],
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: 500,
            memory_bytes: 128 * 1024 * 1024,
            pids: 128,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: Some(60_000),
        },
        isolation: IsolationLevel::Container,
        health: None,
        restart: RestartPolicy::Never,
        outputs: vec![RuntimeOutputSpec {
            name: "oci-layout".into(),
            path: "/outputs/oci-layout".into(),
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            max_bytes: 1024 * 1024,
        }],
        semantics_profile_digest: None,
    }
}

fn cloud_artifact(bytes: &[u8]) -> ArtifactRef {
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    ArtifactRef {
        uri: artifact_uri(&digest).expect("artifact URI"),
        digest,
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    }
}

async fn assert_download_status(
    client: &reqwest::Client,
    endpoint: &str,
    request: &NodeArtifactDownloadRequest,
    expected: reqwest::StatusCode,
) {
    let response = client
        .get(endpoint)
        .query(request)
        .send()
        .await
        .expect("artifact download rejection");
    assert_eq!(response.status(), expected);
}

async fn send_upload(
    client: &reqwest::Client,
    endpoint: &str,
    request: &NodeArtifactUploadRequest,
    bytes: &[u8],
) -> (reqwest::StatusCode, NodeArtifactUploadReceipt) {
    let response = client
        .put(endpoint)
        .query(request)
        .header(reqwest::header::CONTENT_TYPE, &request.media_type)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("artifact upload");
    let status = response.status();
    let body = response.bytes().await.expect("artifact upload body");
    assert!(
        status.is_success(),
        "artifact upload failed: {}",
        String::from_utf8_lossy(&body)
    );
    (
        status,
        serde_json::from_slice(&body).expect("artifact upload receipt"),
    )
}

async fn assert_upload_status(
    client: &reqwest::Client,
    endpoint: &str,
    request: &NodeArtifactUploadRequest,
    bytes: &[u8],
    expected: reqwest::StatusCode,
) {
    let response = client
        .put(endpoint)
        .query(request)
        .header(reqwest::header::CONTENT_TYPE, &request.media_type)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("artifact upload rejection");
    assert_eq!(response.status(), expected);
}
