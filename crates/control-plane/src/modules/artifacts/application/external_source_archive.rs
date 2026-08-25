use super::{BuildInputPreparationError, NodeArtifactReader};
use crate::modules::shared_kernel::domain::{
    BuildRunId, GitCommitSha, OrganizationId, Sha256Digest,
};
use crate::modules::sources::published::GitRepository;
use async_trait::async_trait;

/// Exact request for Sources to materialize one immutable external build input.
///
/// The request contains only published source identity and shared references;
/// Artifacts never receives a Source aggregate, provider credential, checkout,
/// or filesystem location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalSourceArchiveRequest {
    organization_id: OrganizationId,
    build_run_id: BuildRunId,
    repository: GitRepository,
    commit_sha: GitCommitSha,
}

impl ExternalSourceArchiveRequest {
    pub fn new(
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
        repository: GitRepository,
        commit_sha: GitCommitSha,
    ) -> Result<Self, String> {
        let request = Self {
            organization_id,
            build_run_id,
            repository,
            commit_sha,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        let canonical_repository =
            GitRepository::parse(self.repository.provider(), self.repository.canonical_url())?;
        if self.organization_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || canonical_repository != self.repository
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
        {
            return Err("external Source archive request identity is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn build_run_id(&self) -> BuildRunId {
        self.build_run_id
    }

    pub const fn repository(&self) -> &GitRepository {
        &self.repository
    }

    pub const fn commit_sha(&self) -> &GitCommitSha {
        &self.commit_sha
    }
}

/// Open immutable archive produced by the Sources-owned provider adapter.
///
/// The source-content digest identifies the credential-free checkout; the
/// archive digest identifies the exact streamed tar bytes admitted by
/// Artifacts. Keeping both prevents a transport digest from replacing source
/// provenance.
pub struct OpenExternalSourceArchive {
    source_content_digest: Sha256Digest,
    archive_digest: Sha256Digest,
    size_bytes: u64,
    reader: NodeArtifactReader,
}

impl OpenExternalSourceArchive {
    pub fn new(
        source_content_digest: Sha256Digest,
        archive_digest: Sha256Digest,
        size_bytes: u64,
        reader: NodeArtifactReader,
    ) -> Result<Self, String> {
        let archive = Self {
            source_content_digest,
            archive_digest,
            size_bytes,
            reader,
        };
        archive.validate()?;
        Ok(archive)
    }

    pub fn validate(&self) -> Result<(), String> {
        Sha256Digest::parse(self.source_content_digest.as_str())?;
        Sha256Digest::parse(self.archive_digest.as_str())?;
        if self.size_bytes == 0 {
            return Err("external Source archive must contain bytes".into());
        }
        Ok(())
    }

    pub const fn source_content_digest(&self) -> &Sha256Digest {
        &self.source_content_digest
    }

    pub const fn archive_digest(&self) -> &Sha256Digest {
        &self.archive_digest
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub fn into_reader(self) -> NodeArtifactReader {
        self.reader
    }
}

/// Consumer-owned port implemented by the Sources provider adapter.
///
/// `prepare` may create ephemeral provider state, while `remove` must be
/// idempotent. Neither operation grants Artifacts access to that state.
#[async_trait]
pub trait IExternalSourceArchivePort: Send + Sync {
    async fn prepare(
        &self,
        request: ExternalSourceArchiveRequest,
    ) -> Result<OpenExternalSourceArchive, BuildInputPreparationError>;

    async fn remove(&self, build_run_id: BuildRunId) -> Result<(), BuildInputPreparationError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::sources::published::GitProvider;
    use std::io::Cursor;

    #[test]
    fn request_and_archive_reject_noncanonical_identity() {
        let repository =
            GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")
                .expect("repository");
        assert!(ExternalSourceArchiveRequest::new(
            OrganizationId::new(),
            BuildRunId::new(),
            repository,
            GitCommitSha::parse("a".repeat(40)).expect("commit"),
        )
        .is_ok());
        assert!(OpenExternalSourceArchive::new(
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("source digest"),
            Sha256Digest::parse(format!("sha256:{}", "c".repeat(64))).expect("archive digest"),
            0,
            Box::pin(Cursor::new(Vec::new())),
        )
        .is_err());
    }
}
