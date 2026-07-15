use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeCapabilities {
    provider_id: String,
    provider_build: String,
    digest: String,
    document: Value,
}

impl<'de> Deserialize<'de> for NodeCapabilities {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct StoredCapabilities {
            provider_id: String,
            provider_build: String,
            digest: String,
            document: Value,
        }

        let stored = StoredCapabilities::deserialize(deserializer)?;
        let capabilities = Self::new(stored.provider_id, stored.provider_build, stored.document)
            .map_err(serde::de::Error::custom)?;
        if capabilities.digest != stored.digest {
            return Err(serde::de::Error::custom(
                "stored node capabilities digest does not match its document",
            ));
        }
        Ok(capabilities)
    }
}

impl NodeCapabilities {
    pub fn new(
        provider_id: impl Into<String>,
        provider_build: impl Into<String>,
        document: Value,
    ) -> Result<Self, String> {
        let provider_id = provider_id.into();
        let provider_build = provider_build.into();
        validate_identifier("provider ID", &provider_id, 64)?;
        validate_single_line("provider build", &provider_build, 255)?;
        if !document.is_object() {
            return Err("node capabilities must be a JSON object".into());
        }
        let encoded = serde_json::to_vec(&document)
            .map_err(|error| format!("could not encode node capabilities: {error}"))?;
        if encoded.len() > 1024 * 1024 {
            return Err("node capabilities exceed one MiB".into());
        }
        Ok(Self {
            provider_id,
            provider_build,
            digest: format!("sha256:{:x}", Sha256::digest(&encoded)),
            document,
        })
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn provider_build(&self) -> &str {
        &self.provider_build
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn document(&self) -> &Value {
        &self.document
    }
}

fn validate_identifier(label: &str, value: &str, max: usize) -> Result<(), String> {
    validate_single_line(label, value, max)?;
    if value
        .bytes()
        .any(|byte| !(byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)))
    {
        return Err(format!("{label} contains unsupported characters"));
    }
    Ok(())
}

fn validate_single_line(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max || value.contains('\0') || value.contains(['\r', '\n'])
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}
