use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};

const PROFILE_BLOCK: &str = "durable_cell_publisher_profile";
const PROFILE_LABEL: &str = "celld_v0_2_1";
const PROFILE_FIELDS: [&str; 14] = [
    "access_key_environment",
    "adapter_protocol",
    "bundle_mount",
    "command",
    "cpu_millis",
    "ephemeral_storage_bytes",
    "image_digest",
    "image_uri",
    "memory_bytes",
    "pids",
    "schema",
    "secret_access_key_environment",
    "session_token_environment",
    "timeout_ms",
];

pub const DURABLE_CELL_PUBLISHER_PROFILE_SCHEMA: &str = "cloud.durable-cell.publisher-profile.v1";
pub const DURABLE_CELL_PUBLISHER_ADAPTER_PROTOCOL: &str = "celld.deploy.v0.2.1";
pub const DURABLE_CELL_PUBLISHER_PROFILE_MAX_ACL_BYTES: usize = 16 * 1024;

/// Reviewed immutable adapter used only to translate A3S-owned application
/// and S0 ACL into the pinned provider's deploy command. It owns no product
/// configuration, publication lifecycle, credential cache, or object client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellPublisherProfile {
    canonical_acl: String,
    digest: Sha256Digest,
    image_uri: String,
    image_digest: Sha256Digest,
    command: Vec<String>,
    bundle_mount: String,
    cpu_millis: u64,
    memory_bytes: u64,
    pids: u32,
    ephemeral_storage_bytes: u64,
    timeout_ms: u64,
    access_key_environment: String,
    secret_access_key_environment: String,
    session_token_environment: String,
}

impl DurableCellPublisherProfile {
    pub fn pinned_celld_v0_2_1() -> Result<Self, String> {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/cell0.5/celld-v0.2.1-publisher-profile.acl"
        ));
        let canonical = source.strip_suffix('\n').ok_or_else(|| {
            "Durable Cell publisher profile source must end with one newline".to_owned()
        })?;
        if canonical.ends_with(['\r', '\n']) {
            return Err(
                "Durable Cell publisher profile source has a non-canonical line ending".into(),
            );
        }
        Self::parse_acl(canonical)
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > DURABLE_CELL_PUBLISHER_PROFILE_MAX_ACL_BYTES {
            return Err("Durable Cell publisher profile ACL size is invalid".into());
        }
        let document = parse_acl(acl)
            .map_err(|error| format!("Durable Cell publisher profile ACL is invalid: {error}"))?;
        let block = exact_block(&document)?;
        require_exact(block, "schema", DURABLE_CELL_PUBLISHER_PROFILE_SCHEMA)?;
        require_exact(
            block,
            "adapter_protocol",
            DURABLE_CELL_PUBLISHER_ADAPTER_PROTOCOL,
        )?;
        let image_digest = Sha256Digest::parse(required_string(block, "image_digest")?)?;
        let image_uri = required_string(block, "image_uri")?;
        if !image_uri.starts_with("oci://")
            || !image_uri.ends_with(&format!("@{}", image_digest.as_str()))
        {
            return Err("Durable Cell publisher image must be digest-pinned OCI".into());
        }
        let command = required_strings(block, "command")?;
        if command.as_slice() != ["/usr/local/bin/celld"] {
            return Err("Durable Cell publisher command changed from the reviewed adapter".into());
        }
        let bundle_mount = required_string(block, "bundle_mount")?;
        if bundle_mount != "/a3s/durable-cell/application" {
            return Err(
                "Durable Cell publisher bundle mount changed from the reviewed adapter".into(),
            );
        }
        let canonical_acl = generate_acl(&document);
        if canonical_acl != acl {
            return Err("Durable Cell publisher profile must be canonical A3S ACL".into());
        }
        let digest = Sha256Digest::parse(canonical_digest(&document).map_err(|error| {
            format!("Durable Cell publisher profile is not canonicalizable: {error}")
        })?)?;
        let profile = Self {
            canonical_acl,
            digest,
            image_uri,
            image_digest,
            command,
            bundle_mount,
            cpu_millis: required_u64(block, "cpu_millis")?,
            memory_bytes: required_u64(block, "memory_bytes")?,
            pids: u32::try_from(required_u64(block, "pids")?)
                .map_err(|_| "Durable Cell publisher pids exceed u32".to_owned())?,
            ephemeral_storage_bytes: required_u64(block, "ephemeral_storage_bytes")?,
            timeout_ms: required_u64(block, "timeout_ms")?,
            access_key_environment: require_adapter_environment(
                block,
                "access_key_environment",
                "AWS_ACCESS_KEY_ID",
            )?,
            secret_access_key_environment: require_adapter_environment(
                block,
                "secret_access_key_environment",
                "AWS_SECRET_ACCESS_KEY",
            )?,
            session_token_environment: require_adapter_environment(
                block,
                "session_token_environment",
                "AWS_SESSION_TOKEN",
            )?,
        };
        profile.validate_fields()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if &Self::parse_acl(&self.canonical_acl)? != self {
            return Err("Durable Cell publisher profile drifted from canonical ACL".into());
        }
        self.validate_fields()
    }

    fn validate_fields(&self) -> Result<(), String> {
        if self.cpu_millis == 0
            || self.memory_bytes == 0
            || self.pids == 0
            || self.ephemeral_storage_bytes == 0
            || self.timeout_ms == 0
            || self.timeout_ms > crate::modules::executions::domain::MAX_EXECUTION_TIMEOUT_MS
        {
            return Err("Durable Cell publisher profile is invalid".into());
        }
        Ok(())
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn image_uri(&self) -> &str {
        &self.image_uri
    }

    pub const fn image_digest(&self) -> &Sha256Digest {
        &self.image_digest
    }

    pub fn command(&self) -> &[String] {
        &self.command
    }

    pub fn bundle_mount(&self) -> &str {
        &self.bundle_mount
    }

    pub const fn cpu_millis(&self) -> u64 {
        self.cpu_millis
    }

    pub const fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }

    pub const fn pids(&self) -> u32 {
        self.pids
    }

    pub const fn ephemeral_storage_bytes(&self) -> u64 {
        self.ephemeral_storage_bytes
    }

    pub const fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    pub fn access_key_environment(&self) -> &str {
        &self.access_key_environment
    }

    pub fn secret_access_key_environment(&self) -> &str {
        &self.secret_access_key_environment
    }

    pub fn session_token_environment(&self) -> &str {
        &self.session_token_environment
    }
}

fn exact_block(document: &Document) -> Result<&Block, String> {
    if document.blocks.len() != 1 {
        return Err("Durable Cell publisher profile must contain exactly one block".into());
    }
    let block = &document.blocks[0];
    if block.name != PROFILE_BLOCK
        || block.labels.as_slice() != [PROFILE_LABEL]
        || !block.blocks.is_empty()
        || block.attributes.len() != PROFILE_FIELDS.len()
        || block
            .attributes
            .keys()
            .any(|field| !PROFILE_FIELDS.contains(&field.as_str()))
    {
        return Err("Durable Cell publisher profile block shape is invalid".into());
    }
    Ok(block)
}

fn required_string(block: &Block, field: &str) -> Result<String, String> {
    block
        .attributes
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("Durable Cell publisher profile field {field:?} must be a string"))
}

fn required_strings(block: &Block, field: &str) -> Result<Vec<String>, String> {
    let Some(Value::List(values)) = block.attributes.get(field) else {
        return Err(format!(
            "Durable Cell publisher profile field {field:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                format!("Durable Cell publisher profile field {field:?} must be a string list")
            })
        })
        .collect()
}

fn required_u64(block: &Block, field: &str) -> Result<u64, String> {
    let value = block
        .attributes
        .get(field)
        .and_then(Value::as_number)
        .ok_or_else(|| {
            format!("Durable Cell publisher profile field {field:?} must be an integer")
        })?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value <= 0.0
        || value > 9_007_199_254_740_991_f64
    {
        return Err(format!(
            "Durable Cell publisher profile field {field:?} must be a positive safe integer"
        ));
    }
    Ok(value as u64)
}

fn require_exact(block: &Block, field: &str, expected: &str) -> Result<(), String> {
    if required_string(block, field)? != expected {
        return Err(format!(
            "Durable Cell publisher profile field {field:?} must be exactly {expected:?}"
        ));
    }
    Ok(())
}

fn require_adapter_environment(
    block: &Block,
    field: &str,
    expected: &str,
) -> Result<String, String> {
    require_exact(block, field, expected)?;
    Ok(expected.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_profile_is_canonical_digest_bound_acl() {
        let profile =
            DurableCellPublisherProfile::pinned_celld_v0_2_1().expect("pinned publisher profile");
        profile.validate().expect("valid publisher profile");
        assert!(profile
            .image_uri()
            .ends_with(profile.image_digest().as_str()));
        assert_eq!(profile.command(), ["/usr/local/bin/celld"]);
        assert_eq!(profile.access_key_environment(), "AWS_ACCESS_KEY_ID");
        assert_eq!(
            profile.secret_access_key_environment(),
            "AWS_SECRET_ACCESS_KEY"
        );
        assert_eq!(profile.session_token_environment(), "AWS_SESSION_TOKEN");
        assert!(profile
            .canonical_acl()
            .contains(DURABLE_CELL_PUBLISHER_PROFILE_SCHEMA));
    }

    #[test]
    fn profile_rejects_unknown_or_noncanonical_fields() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/cell0.5/celld-v0.2.1-publisher-profile.acl"
        ));
        let canonical = source.strip_suffix('\n').expect("fixture newline");
        assert!(DurableCellPublisherProfile::parse_acl(&canonical.replace(
            "  timeout_ms = 600000\n",
            "  timeout_ms = 600000\n  extra = true\n"
        ))
        .is_err());
        assert!(DurableCellPublisherProfile::parse_acl(&format!("\n{canonical}")).is_err());
    }
}
