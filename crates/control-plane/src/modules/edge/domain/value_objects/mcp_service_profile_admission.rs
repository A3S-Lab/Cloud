use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::{canonical_digest, parse_acl, Document, Value};

/// Edge-owned admission fact for an Assets MCP Service profile.
///
/// Route-policy Domain only needs the digest and the bound ceilings used by
/// path/byte/timeout validation. Full Assets profile ACL reconstruction stays
/// behind Infrastructure anti-corruption adapters for live admission; stored
/// route-policy hydration restores only these Edge-owned fields from the
/// published profile ACL bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeMcpServiceProfileAdmission {
    digest: Sha256Digest,
    endpoint_path: String,
    max_request_bytes: u64,
    max_response_bytes: u64,
    max_stream_seconds: u64,
}

const MCP_SERVICE_PROFILE_MAX_ACL_BYTES: usize = 64 * 1024;
const PROFILE_BLOCK: &str = "mcp_service_profile";
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

impl EdgeMcpServiceProfileAdmission {
    pub fn new(
        digest: Sha256Digest,
        endpoint_path: impl Into<String>,
        max_request_bytes: u64,
        max_response_bytes: u64,
        max_stream_seconds: u64,
    ) -> Result<Self, String> {
        let admission = Self {
            digest,
            endpoint_path: endpoint_path.into(),
            max_request_bytes,
            max_response_bytes,
            max_stream_seconds,
        };
        admission.validate()?;
        Ok(admission)
    }

    /// Restore Edge admission fields from a stored Assets MCP Service profile ACL.
    ///
    /// This verifies the stored digest against the canonical ACL document and
    /// extracts only the fields Edge route-policy Domain consumes. It does not
    /// reconstruct Assets aggregate authority.
    pub fn restore_from_stored_acl(acl: &str, stored_digest: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > MCP_SERVICE_PROFILE_MAX_ACL_BYTES {
            return Err("MCP Service profile ACL size is invalid".into());
        }
        let document = parse_acl(acl)
            .map_err(|error| format!("MCP Service profile ACL is invalid: {error}"))?;
        let digest =
            Sha256Digest::parse(canonical_digest(&document).map_err(|error| {
                format!("MCP Service profile is not canonicalizable: {error}")
            })?)?;
        if digest.as_str() != stored_digest {
            return Err("stored MCP Service profile ACL and digest do not match".into());
        }
        let block = exact_profile_block(&document)?;
        Self::new(
            digest,
            required_string(block, "endpoint_path")?,
            required_u64(block, "max_request_bytes")?,
            required_u64(block, "max_response_bytes")?,
            required_u64(block, "max_stream_seconds")?,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.endpoint_path.is_empty()
            || !self.endpoint_path.starts_with('/')
            || self.endpoint_path.contains("//")
            || self
                .endpoint_path
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err("MCP Service profile admission endpoint path is invalid".into());
        }
        if self.max_request_bytes == 0
            || self.max_response_bytes == 0
            || self.max_stream_seconds == 0
        {
            return Err("MCP Service profile admission bounds must be positive".into());
        }
        Ok(())
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn endpoint_path(&self) -> &str {
        &self.endpoint_path
    }

    pub const fn max_request_bytes(&self) -> u64 {
        self.max_request_bytes
    }

    pub const fn max_response_bytes(&self) -> u64 {
        self.max_response_bytes
    }

    pub const fn max_stream_seconds(&self) -> u64 {
        self.max_stream_seconds
    }
}

fn exact_profile_block(document: &Document) -> Result<&a3s_acl::Block, String> {
    if document.blocks.len() != 1 {
        return Err("MCP Service profile must contain exactly one top-level block".into());
    }
    let block = &document.blocks[0];
    if block.name != PROFILE_BLOCK || !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err("MCP Service profile block shape is invalid".into());
    }
    Ok(block)
}

fn required_value<'a>(block: &'a a3s_acl::Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("MCP Service profile field {name:?} is required"))
}

fn required_string(block: &a3s_acl::Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("MCP Service profile field {name:?} must be a string"))
}

fn required_u64(block: &a3s_acl::Block, name: &str) -> Result<u64, String> {
    let number = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("MCP Service profile field {name:?} must be an integer"))?;
    if !number.is_finite()
        || number.fract() != 0.0
        || number <= 0.0
        || number > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "MCP Service profile field {name:?} must be a positive bounded integer"
        ));
    }
    Ok(number as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_acl::builder::{integer, string, BlockBuilder};
    use a3s_acl::generate_acl;

    fn fixture_acl() -> (String, String) {
        let document = Document {
            blocks: vec![BlockBuilder::new(PROFILE_BLOCK)
                .attr("endpoint_path", string("/mcp"))
                .attr("max_request_bytes", integer(1_048_576))
                .attr("max_response_bytes", integer(4_194_304))
                .attr("max_stream_seconds", integer(60))
                .build()],
        };
        let acl = generate_acl(&document);
        let digest = canonical_digest(&parse_acl(&acl).expect("parse generated")).expect("digest");
        (acl, digest)
    }

    #[test]
    fn restore_from_stored_acl_admits_endpoint_and_bounds() {
        let (acl, digest) = fixture_acl();
        let admission = EdgeMcpServiceProfileAdmission::restore_from_stored_acl(&acl, &digest)
            .expect("restore Edge admission");
        assert_eq!(admission.digest().as_str(), digest);
        assert_eq!(admission.endpoint_path(), "/mcp");
        assert_eq!(admission.max_request_bytes(), 1_048_576);
        assert_eq!(admission.max_response_bytes(), 4_194_304);
        assert_eq!(admission.max_stream_seconds(), 60);
    }

    #[test]
    fn restore_from_stored_acl_rejects_digest_mismatch() {
        let (acl, _) = fixture_acl();
        let error = EdgeMcpServiceProfileAdmission::restore_from_stored_acl(
            &acl,
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .expect_err("digest mismatch");
        assert!(error.contains("do not match"));
    }
}
