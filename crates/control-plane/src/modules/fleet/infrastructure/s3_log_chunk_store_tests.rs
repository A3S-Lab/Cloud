use super::*;
use crate::infrastructure::{
    DisposableS3TestContext, ImmutableObjectClient, S3ImmutableObjectOptions,
};
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogStream};
use object_store::memory::InMemory;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

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
    let client =
        ImmutableObjectClient::from_store(Arc::clone(&objects), "logs").expect("object client");
    let store = LogChunkObjectStore::from_client(client);
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

    let path = ObjectPath::parse(format!("logs/{}", first.object_key)).expect("stored path");
    objects
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
    assert!(ImmutableObjectClient::s3(invalid).is_err());

    let mut invalid = options("https://objects.example");
    invalid.connect_timeout = Duration::from_secs(31);
    assert!(ImmutableObjectClient::s3(invalid).is_err());
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable S3-compatible bucket"]
async fn real_s3_compatible_store_preserves_immutable_log_semantics() {
    let context = DisposableS3TestContext::from_environment("immutable-logs")
        .expect("disposable S3 test context");
    let client = context.client();
    let store = LogChunkObjectStore::from_client(client.clone());
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
    client
        .overwrite_remote_for_test(&first.object_key, b"{\"corrupt\":true}".to_vec())
        .await
        .expect("corrupt real S3 log object");
    assert_eq!(
        store
            .get(&first.object_key, &report("real-s3").checksum)
            .await
            .expect("read corrupt real S3 log object"),
        RetrievedLogChunk::Corrupt
    );
    assert!(matches!(
        store
            .put(Uuid::now_v7(), node_id, 0, &report("real-s3"))
            .await,
        Err(LogChunkStoreError::Conflict(_))
    ));
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

fn options(endpoint: &str) -> S3ImmutableObjectOptions {
    S3ImmutableObjectOptions {
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
