use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_form_core::{
    canonical_sha256, canonicalize_value, parse_json_with_limit, ABSOLUTE_MAX_DOCUMENT_BYTES,
};
use serde::{Deserialize, Serialize};

pub const CLOUD_FORM_DOCUMENT_MAX_BYTES: usize = ABSOLUTE_MAX_DOCUMENT_BYTES as usize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormDocument {
    canonical_json: String,
    digest: Sha256Digest,
}

impl FormDocument {
    pub fn parse(input: &[u8]) -> Result<Self, String> {
        let value = parse_json_with_limit(input, CLOUD_FORM_DOCUMENT_MAX_BYTES)
            .map_err(|error| format!("Form document is invalid JSON: {error}"))?;
        if value.as_object().is_none() {
            return Err("Form document must be a JSON object".into());
        }
        let canonical = canonicalize_value(&value)
            .map_err(|error| format!("Form document could not be canonicalized: {error}"))?;
        if canonical.len() > CLOUD_FORM_DOCUMENT_MAX_BYTES {
            return Err(format!(
                "Form document exceeds its {CLOUD_FORM_DOCUMENT_MAX_BYTES}-byte canonical bound"
            ));
        }
        Self::from_canonical(canonical)
    }

    pub fn restore(canonical_json: String, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse(canonical_json.as_bytes())?;
        if value.canonical_json != canonical_json {
            return Err("stored Form document is not canonical JSON".into());
        }
        if value.digest.as_str() != stored_digest {
            return Err("stored Form document digest does not match its canonical JSON".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(self.canonical_json.clone(), self.digest.as_str())?;
        if restored != *self {
            return Err("stored Form document is invalid".into());
        }
        Ok(())
    }

    pub fn canonical_json(&self) -> &str {
        &self.canonical_json
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    fn from_canonical(canonical: Vec<u8>) -> Result<Self, String> {
        let digest =
            Sha256Digest::parse(format!("sha256:{}", canonical_sha256(canonical.as_slice())))?;
        let canonical_json = String::from_utf8(canonical)
            .map_err(|_| "Form document canonical JSON is not UTF-8".to_owned())?;
        Ok(Self {
            canonical_json,
            digest,
        })
    }
}
