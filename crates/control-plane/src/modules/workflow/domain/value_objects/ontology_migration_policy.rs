use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OntologyMigrationPolicy {
    Initial,
    Compatible,
    Explicit {
        rule_id: String,
        expression_digest: Sha256Digest,
    },
}

impl OntologyMigrationPolicy {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Compatible => "compatible",
            Self::Explicit { .. } => "explicit",
        }
    }

    pub fn rule_id(&self) -> Option<&str> {
        match self {
            Self::Explicit { rule_id, .. } => Some(rule_id),
            Self::Initial | Self::Compatible => None,
        }
    }

    pub fn expression_digest(&self) -> Option<&Sha256Digest> {
        match self {
            Self::Explicit {
                expression_digest, ..
            } => Some(expression_digest),
            Self::Initial | Self::Compatible => None,
        }
    }
}
