use crate::modules::shared_kernel::domain::{BuildRunId, Sha256Digest};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetReleaseProvenance {
    build_run_id: BuildRunId,
    provenance_digest: Sha256Digest,
}

impl AssetReleaseProvenance {
    pub fn new(build_run_id: BuildRunId, provenance_digest: Sha256Digest) -> Result<Self, String> {
        let value = Self {
            build_run_id,
            provenance_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub const fn build_run_id(&self) -> BuildRunId {
        self.build_run_id
    }

    pub const fn provenance_digest(&self) -> &Sha256Digest {
        &self.provenance_digest
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.build_run_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.provenance_digest.as_str())? != self.provenance_digest
        {
            return Err("Asset release provenance identity is invalid".into());
        }
        Ok(())
    }
}
