use super::*;
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogStream};
use object_store::memory::InMemory;
use sha2::{Digest, Sha256};

fn report(data: &str) -> NodeLogChunkReport {
    NodeLogChunkReport {
        unit_id: "service-1".into(),
        generation: 1,
        chunk: RuntimeLogChunk {
            schema: RuntimeLogChunk::SCHEMA.into(),
            cursor: "opaque:cursor:1".into(),
            sequence: 1,
            observed_at_ms: 1,
            stream: RuntimeLogStream::Stdout,
            data: data.into(),
        },
        checksum: format!("sha256:{:x}", Sha256::digest(data.as_bytes())),
    }
}

#[tokio::test]
async fn s3_log_objects_are_immutable_verified_and_retention_safe() {
    let objects: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let store = S3LogChunkStore::from_store(objects, "logs").expect("S3 test store");
    assert!(store.health().await.expect("health"));
    let node_id = Uuid::now_v7();
    let first = store
        .put(Uuid::now_v7(), node_id, 0, &report("hello"))
        .await
        .expect("store first object");
    assert!(first.created);
    let replay = store
        .put(Uuid::now_v7(), node_id, 0, &report("hello"))
        .await
        .expect("replay first object");
    assert!(!replay.created);
    assert_eq!(replay.object_key, first.object_key);
    assert_eq!(
        store
            .get(&first.object_key, &report("hello").checksum)
            .await
            .expect("read first object"),
        RetrievedLogChunk::Found(report("hello"))
    );
    assert!(matches!(
        store
            .put(Uuid::now_v7(), node_id, 0, &report("changed"))
            .await,
        Err(LogChunkStoreError::Conflict(_))
    ));
    assert!(matches!(
        store.remove("../outside").await,
        Err(LogChunkStoreError::Invalid(_))
    ));

    let path = store.object_path(&first.object_key).expect("stored path");
    store
        .objects
        .put(&path, b"{not-json".as_slice().into())
        .await
        .expect("corrupt object");
    assert_eq!(
        store
            .get(&first.object_key, &report("hello").checksum)
            .await
            .expect("read corrupt object"),
        RetrievedLogChunk::Corrupt
    );
    store
        .remove(&first.object_key)
        .await
        .expect("remove object");
    assert_eq!(
        store
            .get(&first.object_key, &report("hello").checksum)
            .await
            .expect("read missing object"),
        RetrievedLogChunk::Missing
    );
    store
        .remove(&first.object_key)
        .await
        .expect("idempotent removal");
}

#[test]
fn s3_options_debug_output_redacts_credentials() {
    let options = options("https://objects.example");
    let debug = format!("{options:?}");
    assert!(!debug.contains("access-key"));
    assert!(!debug.contains("secret-key"));
    assert!(!debug.contains("session-token"));
    assert!(debug.contains("<redacted>"));
}

#[test]
fn s3_options_reject_empty_credentials_and_inverted_timeouts() {
    let mut invalid = options("https://objects.example");
    invalid.secret_access_key.clear();
    assert!(S3LogChunkStore::new(invalid).is_err());

    let mut invalid = options("https://objects.example");
    invalid.connect_timeout = Duration::from_secs(31);
    assert!(S3LogChunkStore::new(invalid).is_err());
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable S3-compatible bucket"]
async fn real_s3_compatible_store_preserves_immutable_log_semantics() {
    let endpoint = std::env::var("A3S_CLOUD_TEST_S3_ENDPOINT").expect("A3S_CLOUD_TEST_S3_ENDPOINT");
    let region =
        std::env::var("A3S_CLOUD_TEST_S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    let bucket = std::env::var("A3S_CLOUD_TEST_S3_BUCKET").expect("A3S_CLOUD_TEST_S3_BUCKET");
    let access_key_id =
        std::env::var("A3S_CLOUD_TEST_S3_ACCESS_KEY_ID").expect("S3 test access key");
    let secret_access_key =
        std::env::var("A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY").expect("S3 test secret key");
    let allow_http = endpoint.starts_with("http://");
    let store = S3LogChunkStore::new(S3LogChunkStoreOptions {
        endpoint: Some(endpoint),
        region,
        bucket,
        prefix: format!("a3s-cloud-tests/{}", Uuid::now_v7()),
        access_key_id,
        secret_access_key,
        session_token: None,
        allow_http,
        virtual_hosted_style: false,
        request_timeout: Duration::from_secs(30),
        connect_timeout: Duration::from_secs(5),
        retry_timeout: Duration::from_secs(60),
        max_retries: 3,
    })
    .expect("real S3 test store");
    assert!(store.health().await.expect("real S3 health"));
    let node_id = Uuid::now_v7();
    let first = store
        .put(Uuid::now_v7(), node_id, 0, &report("real-s3"))
        .await
        .expect("write real S3 log object");
    assert!(first.created);
    assert!(
        !store
            .put(Uuid::now_v7(), node_id, 0, &report("real-s3"))
            .await
            .expect("replay real S3 log object")
            .created
    );
    assert!(matches!(
        store
            .put(Uuid::now_v7(), node_id, 0, &report("changed"))
            .await,
        Err(LogChunkStoreError::Conflict(_))
    ));
    assert_eq!(
        store
            .get(&first.object_key, &report("real-s3").checksum)
            .await
            .expect("read real S3 log object"),
        RetrievedLogChunk::Found(report("real-s3"))
    );
    store
        .remove(&first.object_key)
        .await
        .expect("remove real S3 log object");
    store
        .remove(&first.object_key)
        .await
        .expect("repeat real S3 log object removal");
    assert_eq!(
        store
            .get(&first.object_key, &report("real-s3").checksum)
            .await
            .expect("read retained real S3 log object"),
        RetrievedLogChunk::Missing
    );
}

fn options(endpoint: &str) -> S3LogChunkStoreOptions {
    S3LogChunkStoreOptions {
        endpoint: Some(endpoint.into()),
        region: "us-east-1".into(),
        bucket: "a3s-cloud-logs".into(),
        prefix: "logs".into(),
        access_key_id: "access-key".into(),
        secret_access_key: "secret-key".into(),
        session_token: Some("session-token".into()),
        allow_http: false,
        virtual_hosted_style: false,
        request_timeout: Duration::from_secs(30),
        connect_timeout: Duration::from_secs(5),
        retry_timeout: Duration::from_secs(60),
        max_retries: 3,
    }
}
