use crate::modules::artifacts::domain::BuildEvidence;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct BuildEvidenceResponse(BuildEvidence);

impl From<BuildEvidence> for BuildEvidenceResponse {
    fn from(evidence: BuildEvidence) -> Self {
        Self(evidence)
    }
}
