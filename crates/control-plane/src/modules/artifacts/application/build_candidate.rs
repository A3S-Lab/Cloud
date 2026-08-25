use crate::modules::artifacts::domain::BuildSubject;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GitCommitSha, OrganizationId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Immutable owner-published material retained to distinguish an exact replay
/// from a different request for the same build subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildCandidateEvidence {
    ExternalSourceRevision {
        repository_identity: String,
        commit_sha: GitCommitSha,
        recipe_digest: Sha256Digest,
    },
    HostedAssetRelease {
        commit_sha: GitCommitSha,
        manifest_digest: Sha256Digest,
    },
}

impl BuildCandidateEvidence {
    pub fn external_source_revision(
        repository_identity: String,
        commit_sha: GitCommitSha,
        recipe_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let evidence = Self::ExternalSourceRevision {
            repository_identity,
            commit_sha,
            recipe_digest,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn hosted_asset_release(commit_sha: GitCommitSha, manifest_digest: Sha256Digest) -> Self {
        Self::HostedAssetRelease {
            commit_sha,
            manifest_digest,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Self::ExternalSourceRevision {
            repository_identity,
            ..
        } = self
        {
            if repository_identity.trim().is_empty() || repository_identity.len() > 2_048 {
                return Err("Source build candidate repository identity is invalid".into());
            }
        }
        Ok(())
    }
}

/// Artifacts-owned projection of an owner-published request to create attempt 1.
///
/// This is immutable inbox material, not another lifecycle or queue. A BuildRun
/// remains the only executable build state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCandidate {
    organization_id: OrganizationId,
    subject: BuildSubject,
    evidence: BuildCandidateEvidence,
    requested_at: DateTime<Utc>,
}

impl BuildCandidate {
    pub fn new(
        organization_id: OrganizationId,
        subject: BuildSubject,
        evidence: BuildCandidateEvidence,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let candidate = Self {
            organization_id,
            subject,
            evidence,
            requested_at: canonical_timestamp(requested_at),
        };
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.subject.validate()?;
        self.evidence.validate()?;
        let evidence_matches_subject = matches!(
            (self.subject, &self.evidence),
            (
                BuildSubject::ExternalSourceRevision { .. },
                BuildCandidateEvidence::ExternalSourceRevision { .. }
            ) | (
                BuildSubject::AssetRelease { .. },
                BuildCandidateEvidence::HostedAssetRelease { .. }
            )
        );
        if self.organization_id.as_uuid().is_nil()
            || !evidence_matches_subject
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("build candidate identity, evidence, or request time is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn subject(&self) -> BuildSubject {
        self.subject
    }

    pub const fn evidence(&self) -> &BuildCandidateEvidence {
        &self.evidence
    }

    pub const fn requested_at(&self) -> DateTime<Utc> {
        self.requested_at
    }
}

/// Driven port for the Artifacts-owned immutable candidate read model.
///
/// Implementations must accept exact replays and reject a different fact for
/// an already-projected subject.
#[async_trait]
pub trait IBuildCandidateProjectionPort: Send + Sync {
    async fn project_candidate(&self, candidate: BuildCandidate) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, EnvironmentId, ProjectId, SourceRevisionId,
    };

    #[test]
    fn candidate_reuses_the_build_subject_and_requires_matching_evidence() {
        let source_evidence = BuildCandidateEvidence::external_source_revision(
            "github:github.com/a3s-lab/cloud".into(),
            GitCommitSha::parse("a".repeat(40)).expect("commit"),
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("recipe digest"),
        )
        .expect("Source evidence");
        assert!(BuildCandidate::new(
            OrganizationId::new(),
            BuildSubject::external_source_revision(
                ProjectId::new(),
                EnvironmentId::new(),
                SourceRevisionId::new(),
            ),
            source_evidence.clone(),
            Utc::now(),
        )
        .is_ok());
        assert!(BuildCandidate::new(
            OrganizationId::new(),
            BuildSubject::asset_release(AssetId::new(), AssetReleaseId::new()),
            source_evidence,
            Utc::now(),
        )
        .is_err());
    }
}
