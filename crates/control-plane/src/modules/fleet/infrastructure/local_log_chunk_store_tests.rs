use super::LocalLogChunkStore;
use crate::modules::fleet::domain::services::{ILogChunkStore, LogChunkStoreError};
use a3s_cloud_contracts::NodeLogChunkReport;
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogStream};
use sha2::{Digest, Sha256};
use uuid::Uuid;

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
async fn local_log_objects_are_immutable_idempotent_and_path_safe() {
    let directory = tempfile::tempdir().expect("log directory");
    let store = LocalLogChunkStore::new(directory.path()).expect("log store");
    assert!(store.health().await.expect("health"));
    let batch_id = Uuid::now_v7();
    let node_id = Uuid::now_v7();
    let first = store
        .put(batch_id, node_id, 0, &report("hello"))
        .await
        .expect("store first chunk");
    assert!(first.created);
    let replay = store
        .put(batch_id, node_id, 0, &report("hello"))
        .await
        .expect("replay first chunk");
    assert!(!replay.created);
    assert_eq!(replay.object_key, first.object_key);

    assert!(matches!(
        store.put(batch_id, node_id, 0, &report("changed")).await,
        Err(LogChunkStoreError::Conflict(_))
    ));
    assert!(matches!(
        store.remove("../outside").await,
        Err(LogChunkStoreError::Invalid(_))
    ));
    store
        .remove(&first.object_key)
        .await
        .expect("remove object");
    store
        .remove(&first.object_key)
        .await
        .expect("idempotent removal");
}
