use crate::modules::artifacts::domain::BuildSubject;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GitCommitSha, OrganizationId, PullRequestPreviewId, RepositoryError,
    Sha256Digest,
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
/// This is immutable local fact material, not another delivery or execution
/// lifecycle. A BuildRun remains the only executable build state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCandidate {
    organization_id: OrganizationId,
    subject: BuildSubject,
    preview_id: Option<PullRequestPreviewId>,
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
        Self::new_with_preview(organization_id, subject, None, evidence, requested_at)
    }

    pub fn for_preview_source_revision(
        organization_id: OrganizationId,
        subject: BuildSubject,
        preview_id: PullRequestPreviewId,
        evidence: BuildCandidateEvidence,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::new_with_preview(
            organization_id,
            subject,
            Some(preview_id),
            evidence,
            requested_at,
        )
    }

    fn new_with_preview(
        organization_id: OrganizationId,
        subject: BuildSubject,
        preview_id: Option<PullRequestPreviewId>,
        evidence: BuildCandidateEvidence,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let candidate = Self {
            organization_id,
            subject,
            preview_id,
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
            || self
                .preview_id
                .is_some_and(|preview_id| preview_id.as_uuid().is_nil())
            || self.preview_id.is_some()
                && !matches!(self.subject, BuildSubject::ExternalSourceRevision { .. })
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

    pub const fn preview_id(&self) -> Option<PullRequestPreviewId> {
        self.preview_id
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

        let preview_id = PullRequestPreviewId::new();
        let preview = BuildCandidate::for_preview_source_revision(
            OrganizationId::new(),
            BuildSubject::external_source_revision(
                ProjectId::new(),
                EnvironmentId::new(),
                SourceRevisionId::new(),
            ),
            preview_id,
            BuildCandidateEvidence::external_source_revision(
                "github:github.com/a3s-lab/cloud".into(),
                GitCommitSha::parse("c".repeat(40)).expect("commit"),
                Sha256Digest::parse(format!("sha256:{}", "d".repeat(64))).expect("recipe digest"),
            )
            .expect("Source evidence"),
            Utc::now(),
        )
        .expect("Preview candidate");
        assert_eq!(preview.preview_id(), Some(preview_id));
        assert!(BuildCandidate::for_preview_source_revision(
            OrganizationId::new(),
            BuildSubject::asset_release(AssetId::new(), AssetReleaseId::new()),
            PullRequestPreviewId::new(),
            BuildCandidateEvidence::hosted_asset_release(
                GitCommitSha::parse("e".repeat(40)).expect("commit"),
                Sha256Digest::parse(format!("sha256:{}", "f".repeat(64))).expect("manifest digest"),
            ),
            Utc::now(),
        )
        .is_err());
    }
}
