use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const MAX_INLINE_AGENT_EVENT_BYTES: usize = 64 * 1024;

const _: () = assert!(
    MAX_INLINE_AGENT_EVENT_BYTES == a3s_cloud_contracts::AGENT_PROTOCOL_MAX_EVENT_RECORD_BYTES
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEventContent {
    value: Value,
    digest: Sha256Digest,
    size_bytes: u64,
}

impl AgentEventContent {
    pub fn inline_json(value: Value) -> Result<Self, String> {
        let encoded = serde_json::to_vec(&value)
            .map_err(|error| format!("Agent event content is not serializable: {error}"))?;
        if encoded.len() > MAX_INLINE_AGENT_EVENT_BYTES {
            return Err(format!(
                "inline Agent event content exceeds {MAX_INLINE_AGENT_EVENT_BYTES} bytes"
            ));
        }
        let size_bytes = u64::try_from(encoded.len())
            .map_err(|_| "Agent event content size exceeds the supported range".to_owned())?;
        Ok(Self {
            value,
            digest: Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(encoded)))?,
            size_bytes,
        })
    }

    pub fn restore(value: Value, digest: Sha256Digest, size_bytes: u64) -> Result<Self, String> {
        let content = Self {
            value,
            digest,
            size_bytes,
        };
        content.validate()?;
        Ok(content)
    }

    pub const fn value(&self) -> &Value {
        &self.value
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub fn validate(&self) -> Result<(), String> {
        let rebuilt = Self::inline_json(self.value.clone())?;
        if rebuilt.digest != self.digest || rebuilt.size_bytes != self.size_bytes {
            return Err("Agent event content digest or size changed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_content_is_bounded_and_digest_verified() {
        let content = AgentEventContent::inline_json(serde_json::json!({"message": "hello"}))
            .expect("content");
        assert!(content.digest().as_str().starts_with("sha256:"));
        assert!(content.size_bytes() > 0);
        assert!(AgentEventContent::restore(
            serde_json::json!({"message": "changed"}),
            content.digest().clone(),
            content.size_bytes(),
        )
        .is_err());
        assert!(AgentEventContent::inline_json(Value::String(
            "x".repeat(MAX_INLINE_AGENT_EVENT_BYTES)
        ))
        .is_err());
    }
}
