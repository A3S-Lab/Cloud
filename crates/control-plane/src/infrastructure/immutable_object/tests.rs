use super::*;
use object_store::memory::InMemory;

#[tokio::test]
async fn namespaces_isolate_keys_and_immutable_replays_are_exact() {
    let objects: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let logs =
        ImmutableObjectClient::from_store(Arc::clone(&objects), "logs").expect("log namespace");
    let artifacts =
        ImmutableObjectClient::from_store(objects, "artifacts").expect("artifact namespace");

    assert!(
        logs.put("same-key", b"log".to_vec(), 3)
            .await
            .expect("first log write")
            .created
    );
    assert!(
        !logs
            .put("same-key", b"log".to_vec(), 3)
            .await
            .expect("exact replay")
            .created
    );
    assert!(
        artifacts
            .put("same-key", b"artifact".to_vec(), 8)
            .await
            .expect("isolated artifact write")
            .created
    );
    assert!(matches!(
        logs.put("same-key", b"changed".to_vec(), 7).await,
        Err(ImmutableObjectError::Conflict(_))
    ));
    assert_eq!(
        logs.get("same-key", 3).await.expect("log read"),
        ImmutableObjectRead::Found(b"log".to_vec())
    );
    assert_eq!(
        artifacts.get("same-key", 8).await.expect("artifact read"),
        ImmutableObjectRead::Found(b"artifact".to_vec())
    );
}

#[tokio::test]
async fn local_backend_is_bounded_path_safe_and_idempotently_removable() {
    let directory = tempfile::tempdir().expect("object directory");
    let client =
        ImmutableObjectClient::local(directory.path(), "logs").expect("local object client");
    assert!(client.health().await.expect("local health"));
    assert!(
        client
            .put("records/one", b"content".to_vec(), 7)
            .await
            .expect("local write")
            .created
    );
    assert_eq!(
        client.get("records/one", 6).await.expect("bounded read"),
        ImmutableObjectRead::Corrupt
    );
    assert!(matches!(
        client.get("../outside", 7).await,
        Err(ImmutableObjectError::Invalid(_))
    ));
    client.remove("records/one").await.expect("local removal");
    client
        .remove("records/one")
        .await
        .expect("replayed local removal");
    assert_eq!(
        client.get("records/one", 7).await.expect("missing read"),
        ImmutableObjectRead::Missing
    );
}

#[test]
fn log_adapter_cannot_reimplement_low_level_object_storage() {
    let source = include_str!("../../modules/fleet/infrastructure/log_chunk_object_store.rs");
    for forbidden in [
        "object_store::",
        "std::fs::",
        "tokio::fs::",
        "spawn_blocking",
        "AmazonS3Builder",
        "PutMode::Create",
    ] {
        assert!(
            !source.contains(forbidden),
            "the typed log adapter must reuse ImmutableObjectClient; found {forbidden}"
        );
    }
}
