//! Partner artifact kind admitted by ArtifactAdmission.
//! Cloud records the kind; it does not store partner bytes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartnerArtifactKind {
    Model,
    Git,
    Oci,
    Generic,
}

impl PartnerArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Git => "git",
            Self::Oci => "oci",
            Self::Generic => "generic",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "model" => Ok(Self::Model),
            "git" => Ok(Self::Git),
            "oci" => Ok(Self::Oci),
            "generic" => Ok(Self::Generic),
            _ => Err("partner artifact kind must be model, git, oci, or generic".into()),
        }
    }
}
