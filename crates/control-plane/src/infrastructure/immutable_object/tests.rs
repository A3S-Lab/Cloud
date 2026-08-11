use super::*;
use object_store::memory::InMemory;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use tokio::io::AsyncReadExt;

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn reader(bytes: &[u8]) -> super::stream::ImmutableObjectReader {
    Box::pin(Cursor::new(bytes.to_vec()))
}

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

#[tokio::test]
async fn local_and_remote_streams_verify_replay_and_open_exact_content() {
    let directory = tempfile::tempdir().expect("stream directory");
    let local = ImmutableObjectClient::local(directory.path(), "artifacts").expect("local client");
    let remote = ImmutableObjectClient::from_store(Arc::new(InMemory::new()), "artifacts")
        .expect("remote client");
    let bytes = b"content-addressed stream";
    let digest = digest(bytes);

    for client in [local, remote] {
        assert!(
            client
                .put_stream(
                    "sha256/object",
                    reader(bytes),
                    bytes.len() as u64,
                    &digest,
                    1024,
                )
                .await
                .expect("first stream write")
                .created
        );
        assert!(
            !client
                .put_stream(
                    "sha256/object",
                    reader(bytes),
                    bytes.len() as u64,
                    &digest,
                    1024,
                )
                .await
                .expect("stream replay")
                .created
        );
        assert_eq!(
            client
                .verify("sha256/object", bytes.len() as u64, &digest, 1024)
                .await
                .expect("stream verification"),
            ImmutableObjectVerification::Verified
        );
        let mut opened = match client
            .open("sha256/object", 1024)
            .await
            .expect("open stream")
        {
            ImmutableObjectOpenResult::Found(opened) => opened,
            ImmutableObjectOpenResult::Missing | ImmutableObjectOpenResult::Corrupt => {
                panic!("stored stream was not readable")
            }
        };
        let mut actual = Vec::new();
        opened
            .reader
            .read_to_end(&mut actual)
            .await
            .expect("read stream");
        assert_eq!(actual, bytes);
    }
}

#[tokio::test]
async fn invalid_stream_identity_is_rejected_before_publication() {
    let directory = tempfile::tempdir().expect("stream directory");
    let client = ImmutableObjectClient::local(directory.path(), "artifacts").expect("local client");
    let bytes = b"trusted bytes";
    assert!(matches!(
        client
            .put_stream(
                "sha256/object",
                reader(b"forged bytes!"),
                bytes.len() as u64,
                &digest(bytes),
                1024,
            )
            .await,
        Err(ImmutableObjectError::Integrity(_))
    ));
    assert!(matches!(
        client.open("sha256/object", 1024).await,
        Ok(ImmutableObjectOpenResult::Missing)
    ));
}

#[tokio::test]
async fn remote_streaming_crosses_multipart_boundaries_without_buffering_the_object() {
    let client = ImmutableObjectClient::from_store(Arc::new(InMemory::new()), "artifacts")
        .expect("remote client");
    let bytes = vec![0x5a; 5 * 1024 * 1024 + 17];
    let digest = digest(&bytes);
    assert!(
        client
            .put_stream(
                "sha256/multipart",
                reader(&bytes),
                bytes.len() as u64,
                &digest,
                bytes.len() as u64,
            )
            .await
            .expect("multipart write")
            .created
    );
    assert_eq!(
        client
            .verify(
                "sha256/multipart",
                bytes.len() as u64,
                &digest,
                bytes.len() as u64,
            )
            .await
            .expect("multipart verification"),
        ImmutableObjectVerification::Verified
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

#[test]
fn artifact_adapter_cannot_reimplement_low_level_object_storage() {
    let source =
        include_str!("../../modules/artifacts/infrastructure/local_node_artifact_store.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    for forbidden in [
        "fs2::",
        "std::fs::",
        "tokio::fs::",
        "spawn_blocking",
        "OpenOptions",
        "Sha256",
    ] {
        assert!(
            !production.contains(forbidden),
            "the typed Artifact adapter must reuse ImmutableObjectClient; found {forbidden}"
        );
    }
}

#[test]
fn plugin_trust_root_adapter_cannot_reimplement_low_level_object_storage() {
    let source =
        include_str!("../../modules/plugins/infrastructure/plugin_trust_root_object_store.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    for forbidden in [
        "object_store::",
        "std::fs::",
        "tokio::fs::",
        "spawn_blocking",
        "AmazonS3Builder",
        "PutMode::Create",
    ] {
        assert!(
            !production.contains(forbidden),
            "the typed plugin trust-root adapter must reuse ImmutableObjectClient; found {forbidden}"
        );
    }
}
