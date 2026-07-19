use crate::modules::fleet::domain::services::{LogChunkStoreError, RetrievedLogChunk};
use a3s_cloud_contracts::NodeLogChunkReport;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use uuid::Uuid;

// A valid one MiB text chunk can expand to six bytes per byte when JSON escaped.
pub(super) const MAX_LOG_OBJECT_BYTES: u64 = 8 * 1024 * 1024;

pub(super) fn prepare_log_object(
    node_id: Uuid,
    report: &NodeLogChunkReport,
) -> Result<(String, Vec<u8>), LogChunkStoreError> {
    report.validate().map_err(LogChunkStoreError::Invalid)?;
    if node_id.is_nil() {
        return Err(LogChunkStoreError::Invalid(
            "node ID must not be nil".into(),
        ));
    }
    let unit_digest = format!("{:x}", Sha256::digest(report.unit_id.as_bytes()));
    let cursor_digest = format!("{:x}", Sha256::digest(report.chunk.cursor.as_bytes()));
    let object_key = format!(
        "nodes/{node_id}/units/{unit_digest}/generations/{}/chunks/{:020}-{cursor_digest}.json",
        report.generation, report.chunk.sequence
    );
    let body = serde_json::to_vec(report)
        .map_err(|error| LogChunkStoreError::Invalid(error.to_string()))?;
    if body.len() as u64 > MAX_LOG_OBJECT_BYTES {
        return Err(LogChunkStoreError::Invalid(
            "serialized log object exceeds the storage bound".into(),
        ));
    }
    Ok((object_key, body))
}

pub(super) fn validate_object_key(object_key: &str) -> Result<(), LogChunkStoreError> {
    let path = Path::new(object_key);
    if object_key.is_empty()
        || object_key.len() > 4096
        || object_key.contains(['\\', '\0', '\r', '\n'])
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(LogChunkStoreError::Invalid(
            "log object key is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_expected_checksum(
    expected_checksum: &str,
) -> Result<(), LogChunkStoreError> {
    if !is_sha256(expected_checksum) {
        return Err(LogChunkStoreError::Invalid(
            "expected log checksum is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn verify_log_object(
    body: &[u8],
    expected_checksum: &str,
) -> Result<RetrievedLogChunk, LogChunkStoreError> {
    validate_expected_checksum(expected_checksum)?;
    if body.len() as u64 > MAX_LOG_OBJECT_BYTES {
        return Ok(RetrievedLogChunk::Corrupt);
    }
    let report = match serde_json::from_slice::<NodeLogChunkReport>(body) {
        Ok(report) => report,
        Err(_) => return Ok(RetrievedLogChunk::Corrupt),
    };
    if report.validate().is_err() || report.checksum != expected_checksum {
        return Ok(RetrievedLogChunk::Corrupt);
    }
    Ok(RetrievedLogChunk::Found(report))
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
