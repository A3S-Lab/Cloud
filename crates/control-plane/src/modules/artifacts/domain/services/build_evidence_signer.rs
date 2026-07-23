use crate::modules::artifacts::domain::BuildEvidenceSigningKey;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBuildEvidenceSignature {
    pub key: BuildEvidenceSigningKey,
    pub signature: Vec<u8>,
}

impl VerifiedBuildEvidenceSignature {
    pub fn new(
        key: BuildEvidenceSigningKey,
        signature: Vec<u8>,
    ) -> Result<Self, BuildEvidenceSigningError> {
        key.validate()
            .map_err(BuildEvidenceSigningError::Rejected)?;
        if signature.len() != 64 {
            return Err(BuildEvidenceSigningError::Rejected(
                "Ed25519 build evidence signature must contain exactly 64 bytes".into(),
            ));
        }
        Ok(Self { key, signature })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildEvidenceSigningError {
    #[error("build evidence signing input is invalid: {0}")]
    Invalid(String),
    #[error("build evidence signer is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("build evidence signer rejected or failed verification: {0}")]
    Rejected(String),
}

#[async_trait]
pub trait IBuildEvidenceSigner: Send + Sync {
    async fn sign(
        &self,
        pae: &[u8],
    ) -> Result<VerifiedBuildEvidenceSignature, BuildEvidenceSigningError>;
}
