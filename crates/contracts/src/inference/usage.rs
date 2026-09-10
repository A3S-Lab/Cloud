//! Gateway-to-Cloud usage ingestion contracts.
//!
//! These types define only the authenticated batch and receipt envelope. The
//! Inference module owns ledger persistence and acknowledgement application;
//! this crate remains independent of HTTP, mTLS, and database adapters.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;

pub const INFERENCE_USAGE_BATCH_SCHEMA_V1: &str = "a3s.gateway.usage-batch.v1";
pub const INFERENCE_USAGE_RECEIPT_SCHEMA_V1: &str = "a3s.gateway.usage-batch-receipt.v1";
pub const INFERENCE_USAGE_MAX_RECORDS: usize = 512;
pub const INFERENCE_USAGE_MAX_EVENT_BYTES: usize = 64 * 1024;
pub const INFERENCE_USAGE_MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
pub const INFERENCE_USAGE_MAX_RECEIPT_BYTES: usize = 1024 * 1024;

/// Gateway-local durable position carried across the Cloud ingestion boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsageCursorV1 {
    pub boot_epoch: Uuid,
    pub sequence: u64,
}

impl InferenceUsageCursorV1 {
    fn validate(self, label: &str) -> Result<(), String> {
        if self.boot_epoch.is_nil() || self.sequence == 0 || self.sequence == u64::MAX {
            return Err(format!("{label} cursor is invalid"));
        }
        Ok(())
    }
}

/// One ordered, prompt-free usage event in an ingestion batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsageRecordV1 {
    pub cursor: InferenceUsageCursorV1,
    pub event_id: Uuid,
    pub payload_base64: String,
    pub payload_sha256: String,
}

impl InferenceUsageRecordV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("usage event ID", self.event_id)?;
        self.cursor.validate("usage event")?;
        let payload = decode_payload(&self.payload_base64)?;
        if payload.len() > INFERENCE_USAGE_MAX_EVENT_BYTES {
            return Err(format!(
                "usage event exceeds {} bytes",
                INFERENCE_USAGE_MAX_EVENT_BYTES
            ));
        }
        validate_sha256("usage event payload", &self.payload_sha256)?;
        let expected = format!("{:x}", Sha256::digest(&payload));
        if self.payload_sha256 != expected {
            return Err("usage event payload SHA-256 does not match its bytes".into());
        }
        Ok(())
    }

    pub fn payload(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        decode_payload(&self.payload_base64)
    }
}

/// Transport-neutral Gateway usage batch accepted by Cloud.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsageBatchV1 {
    pub schema: String,
    pub gateway_id: Uuid,
    pub batch_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<InferenceUsageCursorV1>,
    pub records: Vec<InferenceUsageRecordV1>,
}

impl InferenceUsageBatchV1 {
    pub const SCHEMA: &'static str = INFERENCE_USAGE_BATCH_SCHEMA_V1;

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported inference usage batch schema {:?}",
                self.schema
            ));
        }
        validate_uuid("usage batch Gateway ID", self.gateway_id)?;
        validate_uuid("usage batch ID", self.batch_id)?;
        if let Some(after) = self.after {
            after.validate("usage batch after")?;
        }
        if self.records.is_empty() || self.records.len() > INFERENCE_USAGE_MAX_RECORDS {
            return Err(format!(
                "inference usage batch must contain 1 to {} records",
                INFERENCE_USAGE_MAX_RECORDS
            ));
        }

        let mut total_payload_bytes = 0_usize;
        let mut cursors = HashSet::new();
        let mut epochs = HashSet::new();
        let mut current_epoch = None;
        let mut last_sequence: Option<u64> = None;
        let mut event_ids = HashSet::new();
        for (index, record) in self.records.iter().enumerate() {
            record.validate()?;
            if self.after == Some(record.cursor) {
                return Err("inference usage batch repeats its after cursor".into());
            }
            // Only the first record must immediately follow `after`. Later
            // records are constrained by same-epoch contiguity below. Matching
            // Gateway `UsageIngestBatch::validate` keeps the Cloud contract from
            // rejecting lawful multi-record continuations.
            if index == 0 {
                if let Some(after) = self.after {
                    if after.boot_epoch == record.cursor.boot_epoch {
                        let expected = after
                            .sequence
                            .checked_add(1)
                            .ok_or_else(|| "usage batch after cursor overflows".to_string())?;
                        if record.cursor.sequence != expected {
                            return Err(
                                "inference usage batch does not immediately follow its after cursor"
                                    .into(),
                            );
                        }
                    }
                }
            }
            if !cursors.insert(record.cursor) {
                return Err("inference usage batch contains duplicate cursors".into());
            }
            if current_epoch != Some(record.cursor.boot_epoch) {
                if !epochs.insert(record.cursor.boot_epoch) {
                    return Err("inference usage batch revisits a boot epoch".into());
                }
                current_epoch = Some(record.cursor.boot_epoch);
                last_sequence = None;
            }
            if let Some(sequence) = last_sequence {
                let expected = sequence
                    .checked_add(1)
                    .ok_or_else(|| "usage batch cursor sequence overflows".to_string())?;
                if record.cursor.sequence != expected {
                    return Err("inference usage batch cursor sequence is not contiguous".into());
                }
            }
            last_sequence = Some(record.cursor.sequence);
            if !event_ids.insert(record.event_id) {
                return Err("inference usage batch contains duplicate event IDs".into());
            }
            total_payload_bytes = total_payload_bytes
                .checked_add(record.payload()?.len())
                .ok_or_else(|| "inference usage batch payload size overflows".to_string())?;
        }
        if total_payload_bytes > INFERENCE_USAGE_MAX_BATCH_BYTES {
            return Err(format!(
                "inference usage batch exceeds {} bytes of payload",
                INFERENCE_USAGE_MAX_BATCH_BYTES
            ));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode inference usage batch: {error}"))?;
        if encoded.len() > INFERENCE_USAGE_MAX_BATCH_BYTES {
            return Err(format!(
                "encoded inference usage batch exceeds {} bytes",
                INFERENCE_USAGE_MAX_BATCH_BYTES
            ));
        }
        Ok(encoded)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > INFERENCE_USAGE_MAX_BATCH_BYTES {
            return Err(format!(
                "encoded inference usage batch exceeds {} bytes",
                INFERENCE_USAGE_MAX_BATCH_BYTES
            ));
        }
        let batch: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("inference usage batch is invalid JSON: {error}"))?;
        batch.validate()?;
        Ok(batch)
    }
}

/// Cloud's highest-contiguous acknowledgement for one exact usage batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsageReceiptV1 {
    pub schema: String,
    pub gateway_id: Uuid,
    pub batch_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_through: Option<InferenceUsageCursorV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<InferenceUsageCursorV1>,
}

impl InferenceUsageReceiptV1 {
    pub const SCHEMA: &'static str = INFERENCE_USAGE_RECEIPT_SCHEMA_V1;

    pub fn validate_for(&self, batch: &InferenceUsageBatchV1) -> Result<(), String> {
        batch.validate()?;
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported inference usage receipt schema {:?}",
                self.schema
            ));
        }
        if self.gateway_id != batch.gateway_id || self.batch_id != batch.batch_id {
            return Err("inference usage receipt changed batch identity".into());
        }
        if let Some(cursor) = self.acknowledged_through {
            cursor.validate("inference usage acknowledgement")?;
            if self.acknowledged_through != batch.after
                && !batch.records.iter().any(|record| record.cursor == cursor)
            {
                return Err(
                    "inference usage receipt acknowledges a cursor outside its batch".into(),
                );
            }
        }
        if self.gaps.len() > INFERENCE_USAGE_MAX_RECORDS {
            return Err("inference usage receipt contains too many gaps".into());
        }
        let mut gaps = HashSet::new();
        for gap in &self.gaps {
            gap.validate("inference usage gap")?;
            if !gaps.insert(*gap) {
                return Err("inference usage receipt contains duplicate gaps".into());
            }
        }
        if self
            .acknowledged_through
            .is_some_and(|cursor| self.gaps.contains(&cursor))
        {
            return Err("inference usage receipt acknowledges a reported gap".into());
        }
        Ok(())
    }

    pub fn encode_for(&self, batch: &InferenceUsageBatchV1) -> Result<Vec<u8>, String> {
        self.validate_for(batch)?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode inference usage receipt: {error}"))?;
        if encoded.len() > INFERENCE_USAGE_MAX_RECEIPT_BYTES {
            return Err(format!(
                "encoded inference usage receipt exceeds {} bytes",
                INFERENCE_USAGE_MAX_RECEIPT_BYTES
            ));
        }
        Ok(encoded)
    }

    pub fn decode_for(bytes: &[u8], batch: &InferenceUsageBatchV1) -> Result<Self, String> {
        if bytes.len() > INFERENCE_USAGE_MAX_RECEIPT_BYTES {
            return Err(format!(
                "encoded inference usage receipt exceeds {} bytes",
                INFERENCE_USAGE_MAX_RECEIPT_BYTES
            ));
        }
        let receipt: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("inference usage receipt is invalid JSON: {error}"))?;
        receipt.validate_for(batch)?;
        Ok(receipt)
    }
}

fn decode_payload(payload_base64: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(payload_base64.as_bytes())
        .map_err(|error| format!("usage event payload is invalid base64: {error}"))
}

fn validate_sha256(label: &str, value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{label} SHA-256 must be 64 hexadecimal characters"));
    }
    if value.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(format!("{label} SHA-256 must use lowercase hexadecimal"));
    }
    Ok(())
}

fn validate_uuid(label: &str, value: Uuid) -> Result<(), String> {
    if value.is_nil() {
        return Err(format!("{label} must not be the nil UUID"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
