use crate::modules::shared_kernel::domain::Sha256Digest;

/// Edge-owned admission fact for an Assets MCP Service profile.
///
/// Route-policy Domain only needs the digest and the bound ceilings used by
/// path/byte/timeout validation. Full Assets profile ACL reconstruction stays
/// behind Infrastructure anti-corruption adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeMcpServiceProfileAdmission {
    digest: Sha256Digest,
    endpoint_path: String,
    max_request_bytes: u64,
    max_response_bytes: u64,
    max_stream_seconds: u64,
}

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
