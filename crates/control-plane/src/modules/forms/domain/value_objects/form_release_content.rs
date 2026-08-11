use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_form_core::{
    canonicalize_value, digest_document_json, parse_json_with_limit, ABSOLUTE_MAX_DOCUMENT_BYTES,
    ABSOLUTE_MAX_RESPONSE_BYTES, COMPILER_REVISION, FORM_SCHEMA_PROFILE_1,
};
use serde::{Deserialize, Serialize};

pub const CLOUD_FORM_RELEASE_MAX_DOCUMENT_BYTES: usize = ABSOLUTE_MAX_DOCUMENT_BYTES as usize;
pub const CLOUD_FORM_RELEASE_MAX_PLAN_BYTES: usize = ABSOLUTE_MAX_RESPONSE_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormReleaseContent {
    normalized_document_json: String,
    form_plan_json: String,
    compiler_revision: String,
    schema_profile: String,
    digest: Sha256Digest,
}

impl FormReleaseContent {
    pub fn restore(
        normalized_document_json: String,
        form_plan_json: String,
        compiler_revision: String,
        schema_profile: String,
        stored_digest: &str,
    ) -> Result<Self, String> {
        let digest = Sha256Digest::parse(stored_digest)?;
        let normalized = parse_canonical_object(
            "normalized Form document",
            &normalized_document_json,
            CLOUD_FORM_RELEASE_MAX_DOCUMENT_BYTES,
        )?;
        let plan = parse_canonical_object(
            "Form plan",
            &form_plan_json,
            CLOUD_FORM_RELEASE_MAX_PLAN_BYTES,
        )?;

        if compiler_revision != COMPILER_REVISION {
            return Err("Form release compiler revision is not the pinned native revision".into());
        }
        if schema_profile != FORM_SCHEMA_PROFILE_1 {
            return Err("Form release schema profile is not supported".into());
        }
        let computed_digest = digest_document_json(normalized_document_json.as_bytes())
            .map_err(|error| format!("normalized Form document digest is invalid: {error}"))?;
        if computed_digest != digest.as_str()
            || normalized.get("digest").and_then(|value| value.as_str()) != Some(digest.as_str())
        {
            return Err("Form release digest does not match its normalized document".into());
        }
        if plan.get("schemaProfile").and_then(|value| value.as_str())
            != Some(schema_profile.as_str())
            || plan.get("sourceDigest").and_then(|value| value.as_str()) != Some(digest.as_str())
        {
            return Err("Form plan does not match its document or schema profile".into());
        }

        Ok(Self {
            normalized_document_json,
            form_plan_json,
            compiler_revision,
            schema_profile,
            digest,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(
            self.normalized_document_json.clone(),
            self.form_plan_json.clone(),
            self.compiler_revision.clone(),
            self.schema_profile.clone(),
            self.digest.as_str(),
        )?;
        if restored != *self {
            return Err("stored Form release content is invalid".into());
        }
        Ok(())
    }

    pub fn normalized_document_json(&self) -> &str {
        &self.normalized_document_json
    }

    pub fn form_plan_json(&self) -> &str {
        &self.form_plan_json
    }

    pub fn compiler_revision(&self) -> &str {
        &self.compiler_revision
    }

    pub fn schema_profile(&self) -> &str {
        &self.schema_profile
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_canonical_object(
    label: &str,
    input: &str,
    maximum_bytes: usize,
) -> Result<a3s_form_core::CanonicalValue, String> {
    if input.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    let value = parse_json_with_limit(input.as_bytes(), maximum_bytes)
        .map_err(|error| format!("{label} is invalid JSON: {error}"))?;
    if value.as_object().is_none() {
        return Err(format!("{label} must be a JSON object"));
    }
    let canonical = canonicalize_value(&value)
        .map_err(|error| format!("{label} could not be canonicalized: {error}"))?;
    if canonical.as_slice() != input.as_bytes() {
        return Err(format!("{label} must use canonical JSON bytes"));
    }
    Ok(value)
}
