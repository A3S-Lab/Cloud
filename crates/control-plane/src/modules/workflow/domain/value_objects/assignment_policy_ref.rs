use crate::modules::shared_kernel::domain::{sha256_digest, Sha256Digest};
use serde::{Deserialize, Serialize};

const MAX_ASSIGNMENT_POLICY_ID_BYTES: usize = 512;
const MAX_PORTABLE_INTEGER: u64 = 9_007_199_254_740_991;
pub const WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_ID: &str =
    "cloud.workflow.assignment.organization-member-exclusive";
pub const WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_REVISION: u64 = 1;
pub const WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_DIGEST: &str =
    "sha256:2def16b8b86f6e78278ef0699c4fdfb0bba2b6612fb00f7007b22135ed35587a";
const WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_CONTENT: &[u8] = br#"{"claimMode":"exclusive","eligibility":"organization_member_with_project_access","schema":"cloud.workflow.assignment-policy.v1"}"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentPolicyRef {
    pub id: String,
    pub revision: u64,
    pub digest: Sha256Digest,
}

impl AssignmentPolicyRef {
    pub fn new(id: impl Into<String>, revision: u64, digest: Sha256Digest) -> Result<Self, String> {
        let value = Self {
            id: id.into(),
            revision,
            digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.trim() != self.id
            || self.id.len() > MAX_ASSIGNMENT_POLICY_ID_BYTES
            || self.id.contains(['\0', '\r', '\n'])
            || self.revision == 0
            || self.revision > MAX_PORTABLE_INTEGER
        {
            return Err("assignment policy reference is invalid".into());
        }
        Ok(())
    }

    pub fn workflow_organization_member_exclusive() -> Result<Self, String> {
        if sha256_digest(WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_CONTENT)
            != WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_DIGEST
        {
            return Err(
                "built-in Workflow assignment policy changed without a revision update".into(),
            );
        }
        Self::new(
            WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_ID,
            WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_REVISION,
            Sha256Digest::parse(WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_DIGEST)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest")
    }

    #[test]
    fn requires_an_exact_positive_policy_revision() {
        AssignmentPolicyRef::new("approval-policy", 1, digest()).expect("reference");
        assert!(AssignmentPolicyRef::new("approval-policy", 0, digest()).is_err());
        assert!(AssignmentPolicyRef::new(" padded ", 1, digest()).is_err());
    }

    #[test]
    fn built_in_workflow_assignment_policy_is_revision_and_digest_pinned() {
        let policy = AssignmentPolicyRef::workflow_organization_member_exclusive()
            .expect("built-in assignment policy");
        assert_eq!(policy.id, WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_ID);
        assert_eq!(
            policy.revision,
            WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_REVISION
        );
        assert_eq!(
            policy.digest.as_str(),
            WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_DIGEST
        );
    }
}
