use crate::modules::shared_kernel::domain::{
    SecretId, SecretVersionReference, Sha256Digest,
};
use a3s_acl::builder::{integer, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEPLOYMENT_BLOCK: &str = "durable_cell_deployment";
const REQUIRED_ATTRIBUTES: [&str; 11] = [
    "access_key_secret_id",
    "access_key_secret_version",
    "credential_generation",
    "deletion_grace_period_seconds",
    "maximum_recovery_point_age_seconds",
    "maximum_sealed_recovery_points",
    "minimum_sealed_recovery_points",
    "provider_profile_digest",
    "schema",
    "secret_access_key_secret_id",
    "secret_access_key_secret_version",
];
const OPTIONAL_ATTRIBUTES: [&str; 2] = ["session_token_secret_id", "session_token_secret_version"];
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_RECOVERY_POINTS: u32 = 10_000;
const MINIMUM_RECOVERY_POINT_AGE_SECONDS: u64 = 60 * 60;
const MAXIMUM_RECOVERY_POINT_AGE_SECONDS: u64 = 10 * 365 * 24 * 60 * 60;
const MINIMUM_DELETION_GRACE_SECONDS: u64 = 5 * 60;
const MAXIMUM_DELETION_GRACE_SECONDS: u64 = 30 * 24 * 60 * 60;

pub const DURABLE_CELL_DEPLOYMENT_SCHEMA: &str = "cloud.durable-cell.deployment.v1";
pub const DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES: usize = 16 * 1024;

/// Provider-neutral S0 retention values carried by the Durable Cell
/// deployment ACL. Data owns the persisted retention aggregate; this bounded
/// contract keeps the public Durable Cell language independent of that type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellRetentionPolicySpec {
    pub minimum_sealed_recovery_points: u32,
    pub maximum_sealed_recovery_points: u32,
    pub maximum_recovery_point_age_seconds: u64,
    pub deletion_grace_period_seconds: u64,
}

impl DurableCellRetentionPolicySpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.minimum_sealed_recovery_points == 0
            || self.maximum_sealed_recovery_points < self.minimum_sealed_recovery_points
            || self.maximum_sealed_recovery_points > MAX_RECOVERY_POINTS
            || !(MINIMUM_RECOVERY_POINT_AGE_SECONDS..=MAXIMUM_RECOVERY_POINT_AGE_SECONDS)
                .contains(&self.maximum_recovery_point_age_seconds)
            || !(MINIMUM_DELETION_GRACE_SECONDS..=MAXIMUM_DELETION_GRACE_SECONDS)
                .contains(&self.deletion_grace_period_seconds)
        {
            return Err(
                "Durable Cell deployment retention policy is outside supported bounds".into(),
            );
        }
        Ok(())
    }
}

/// Public, plaintext-free deployment bindings for one Durable Cell revision.
/// Tenant and namespace identity are deliberately absent: the Durable Cells
/// owner derives those values from the authenticated URL scope and application
/// identity before delegating to S0 and Workloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellDeploymentBindingSpec {
    pub credential_generation: u64,
    pub provider_profile_digest: Sha256Digest,
    pub access_key_id: SecretVersionReference,
    pub secret_access_key: SecretVersionReference,
    pub session_token: Option<SecretVersionReference>,
    pub retention_policy: DurableCellRetentionPolicySpec,
}

impl DurableCellDeploymentBindingSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.credential_generation == 0
            || self.credential_generation > MAX_SAFE_ACL_INTEGER
            || Sha256Digest::parse(self.provider_profile_digest.as_str())?
                != self.provider_profile_digest
        {
            return Err("Durable Cell deployment binding identity is invalid".into());
        }
        self.access_key_id.validate()?;
        self.secret_access_key.validate()?;
        if let Some(session_token) = self.session_token {
            session_token.validate()?;
        }
        let mut secret_ids = vec![
            self.access_key_id.secret_id,
            self.secret_access_key.secret_id,
        ];
        if let Some(session_token) = self.session_token {
            secret_ids.push(session_token.secret_id);
        }
        secret_ids.sort_unstable();
        secret_ids.dedup();
        if secret_ids.len() != 2 + usize::from(self.session_token.is_some()) {
            return Err(
                "Durable Cell deployment credential fields require distinct Secrets".into(),
            );
        }
        self.retention_policy.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellDeploymentBinding {
    spec: DurableCellDeploymentBindingSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl DurableCellDeploymentBinding {
    pub fn from_spec(spec: DurableCellDeploymentBindingSpec) -> Result<Self, String> {
        spec.validate()?;
        let document = binding_document(&spec)?;
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES {
            return Err("Durable Cell deployment ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated Durable Cell deployment ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("Durable Cell deployment ACL is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES {
            return Err("Durable Cell deployment ACL size is invalid".into());
        }
        let document = parse_acl(acl)
            .map_err(|error| format!("Durable Cell deployment ACL is invalid: {error}"))?;
        Self::from_spec(parse_binding(&document)?)
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let binding = Self::parse_acl(acl)?;
        if binding.canonical_acl != acl || binding.digest.as_str() != stored_digest {
            return Err("stored Durable Cell deployment ACL and digest do not match".into());
        }
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(&self.canonical_acl, self.digest.as_str())?;
        if &restored != self {
            return Err("Durable Cell deployment binding drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &DurableCellDeploymentBindingSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn binding_document(spec: &DurableCellDeploymentBindingSpec) -> Result<Document, String> {
    let mut root = BlockBuilder::new(DEPLOYMENT_BLOCK)
        .attr("schema", string(DURABLE_CELL_DEPLOYMENT_SCHEMA))
        .attr(
            "credential_generation",
            acl_integer("credential generation", spec.credential_generation)?,
        )
        .attr(
            "provider_profile_digest",
            string(spec.provider_profile_digest.as_str()),
        )
        .attr(
            "access_key_secret_id",
            string(&spec.access_key_id.secret_id.to_string()),
        )
        .attr(
            "access_key_secret_version",
            acl_integer("access key Secret version", spec.access_key_id.version)?,
        )
        .attr(
            "secret_access_key_secret_id",
            string(&spec.secret_access_key.secret_id.to_string()),
        )
        .attr(
            "secret_access_key_secret_version",
            acl_integer(
                "secret access key Secret version",
                spec.secret_access_key.version,
            )?,
        )
        .attr(
            "minimum_sealed_recovery_points",
            integer(i64::from(
                spec.retention_policy.minimum_sealed_recovery_points,
            )),
        )
        .attr(
            "maximum_sealed_recovery_points",
            integer(i64::from(
                spec.retention_policy.maximum_sealed_recovery_points,
            )),
        )
        .attr(
            "maximum_recovery_point_age_seconds",
            acl_integer(
                "maximum recovery point age",
                spec.retention_policy.maximum_recovery_point_age_seconds,
            )?,
        )
        .attr(
            "deletion_grace_period_seconds",
            acl_integer(
                "deletion grace period",
                spec.retention_policy.deletion_grace_period_seconds,
            )?,
        );
    if let Some(session_token) = spec.session_token {
        root = root
            .attr(
                "session_token_secret_id",
                string(&session_token.secret_id.to_string()),
            )
            .attr(
                "session_token_secret_version",
                acl_integer("session token Secret version", session_token.version)?,
            );
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

fn parse_binding(document: &Document) -> Result<DurableCellDeploymentBindingSpec, String> {
    if document.blocks.len() != 1 {
        return Err("Durable Cell deployment ACL must contain exactly one block".into());
    }
    let block = &document.blocks[0];
    if block.name != DEPLOYMENT_BLOCK || !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err("Durable Cell deployment ACL block shape is invalid".into());
    }
    for required in REQUIRED_ATTRIBUTES {
        if !block.attributes.contains_key(required) {
            return Err(format!(
                "Durable Cell deployment ACL attribute {required:?} is required"
            ));
        }
    }
    if block.attributes.keys().any(|name| {
        !REQUIRED_ATTRIBUTES.contains(&name.as_str())
            && !OPTIONAL_ATTRIBUTES.contains(&name.as_str())
    }) {
        return Err("Durable Cell deployment ACL contains unsupported attributes".into());
    }
    let session_id = optional_uuid(block, "session_token_secret_id")?;
    let session_version = optional_u64(block, "session_token_secret_version")?;
    if session_id.is_some() != session_version.is_some() {
        return Err(
            "Durable Cell deployment ACL session token identity and version must appear together"
                .into(),
        );
    }
    require_exact_string(block, "schema", DURABLE_CELL_DEPLOYMENT_SCHEMA)?;
    Ok(DurableCellDeploymentBindingSpec {
        credential_generation: required_u64(block, "credential_generation")?,
        provider_profile_digest: Sha256Digest::parse(&required_string(
            block,
            "provider_profile_digest",
        )?)?,
        access_key_id: secret_reference(
            block,
            "access_key_secret_id",
            "access_key_secret_version",
        )?,
        secret_access_key: secret_reference(
            block,
            "secret_access_key_secret_id",
            "secret_access_key_secret_version",
        )?,
        session_token: session_id
            .zip(session_version)
            .map(|(id, version)| SecretVersionReference::new(SecretId::from_uuid(id), version))
            .transpose()?,
        retention_policy: DurableCellRetentionPolicySpec {
            minimum_sealed_recovery_points: required_u32(block, "minimum_sealed_recovery_points")?,
            maximum_sealed_recovery_points: required_u32(block, "maximum_sealed_recovery_points")?,
            maximum_recovery_point_age_seconds: required_u64(
                block,
                "maximum_recovery_point_age_seconds",
            )?,
            deletion_grace_period_seconds: required_u64(block, "deletion_grace_period_seconds")?,
        },
    })
}

fn secret_reference(
    block: &Block,
    id_name: &str,
    version_name: &str,
) -> Result<SecretVersionReference, String> {
    SecretVersionReference::new(
        SecretId::from_uuid(required_uuid(block, id_name)?),
        required_u64(block, version_name)?,
    )
}

fn require_exact_string(block: &Block, name: &str, expected: &str) -> Result<(), String> {
    let value = required_string(block, name)?;
    if value != expected {
        return Err(format!(
            "Durable Cell deployment ACL attribute {name:?} must be {expected:?}"
        ));
    }
    Ok(())
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match block.attributes.get(name) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(format!(
            "Durable Cell deployment ACL attribute {name:?} must be a string"
        )),
        None => Err(format!(
            "Durable Cell deployment ACL attribute {name:?} is required"
        )),
    }
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("Durable Cell deployment ACL attribute {name:?} must be a UUID"))
}

fn optional_uuid(block: &Block, name: &str) -> Result<Option<Uuid>, String> {
    block
        .attributes
        .get(name)
        .map(|_| required_uuid(block, name))
        .transpose()
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    match block.attributes.get(name) {
        Some(Value::Number(value))
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= MAX_SAFE_ACL_INTEGER as f64 =>
        {
            Ok(*value as u64)
        }
        Some(_) => Err(format!(
            "Durable Cell deployment ACL attribute {name:?} must be a non-negative safe integer"
        )),
        None => Err(format!(
            "Durable Cell deployment ACL attribute {name:?} is required"
        )),
    }
}

fn optional_u64(block: &Block, name: &str) -> Result<Option<u64>, String> {
    block
        .attributes
        .get(name)
        .map(|_| required_u64(block, name))
        .transpose()
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    u32::try_from(required_u64(block, name)?).map_err(|_| {
        format!("Durable Cell deployment ACL attribute {name:?} exceeds the u32 range")
    })
}

fn acl_integer(label: &str, value: u64) -> Result<Value, String> {
    if value > MAX_SAFE_ACL_INTEGER {
        return Err(format!("{label} exceeds the ACL safe integer range"));
    }
    Ok(integer(value as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OrganizationId, StorageNamespaceId};

    fn reference() -> SecretVersionReference {
        SecretVersionReference::new(SecretId::new(), 1).expect("reference")
    }

    fn spec() -> DurableCellDeploymentBindingSpec {
        DurableCellDeploymentBindingSpec {
            credential_generation: 1,
            provider_profile_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                .expect("digest"),
            access_key_id: reference(),
            secret_access_key: reference(),
            session_token: Some(reference()),
            retention_policy: DurableCellRetentionPolicySpec {
                minimum_sealed_recovery_points: 2,
                maximum_sealed_recovery_points: 24,
                maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
                deletion_grace_period_seconds: 24 * 60 * 60,
            },
        }
    }

    #[test]
    fn canonical_acl_projects_only_authenticated_scope() {
        let binding = DurableCellDeploymentBinding::from_spec(spec()).expect("binding");
        let restored = DurableCellDeploymentBinding::restore(
            binding.canonical_acl(),
            binding.digest().as_str(),
        )
        .expect("canonical binding");
        assert_eq!(restored, binding);

        assert_eq!(binding.spec().credential_generation, 1);
        assert_eq!(
            binding
                .spec()
                .retention_policy
                .maximum_sealed_recovery_points,
            24
        );
        let organization_id = OrganizationId::new();
        let namespace_id = StorageNamespaceId::new();
        assert!(!binding
            .canonical_acl()
            .contains(&organization_id.to_string()));
        assert!(!binding.canonical_acl().contains(&namespace_id.to_string()));
    }

    #[test]
    fn rejects_unknown_fields_partial_session_tokens_and_secret_aliases() {
        let binding = DurableCellDeploymentBinding::from_spec(spec()).expect("binding");
        let unknown = binding
            .canonical_acl()
            .replacen("\n}", "\n  unsupported = true\n}", 1);
        assert!(DurableCellDeploymentBinding::parse_acl(&unknown).is_err());

        let without_version = binding
            .canonical_acl()
            .lines()
            .filter(|line| !line.contains("session_token_secret_version"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(DurableCellDeploymentBinding::parse_acl(&without_version).is_err());

        let mut aliased = spec();
        aliased.secret_access_key = SecretVersionReference::new(
            aliased.access_key_id.secret_id,
            aliased.access_key_id.version + 1,
        )
        .expect("alias");
        assert!(DurableCellDeploymentBinding::from_spec(aliased).is_err());
    }
}
