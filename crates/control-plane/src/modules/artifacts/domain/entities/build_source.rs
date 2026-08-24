use super::BuildSubject;
use crate::modules::shared_kernel::domain::{AssetId, GitCommitSha, OrganizationId, Sha256Digest};
use crate::modules::sources::published::{BuildRecipe, GitRepository, SourceBuildInputSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildSourceLocation {
    ExternalGit { repository: GitRepository },
    HostedAssetGit { asset_id: AssetId },
}

/// A fully resolved immutable source consumed by the one Cloud build workflow.
///
/// This is a read model over the owning Source or Assets context. It is not a
/// second source aggregate and is never persisted as another source table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSource {
    pub organization_id: OrganizationId,
    pub subject: BuildSubject,
    pub location: BuildSourceLocation,
    pub repository: String,
    pub commit_sha: GitCommitSha,
    pub manifest_digest: Option<Sha256Digest>,
    pub recipe: BuildRecipe,
    pub recipe_digest: String,
}

impl BuildSource {
    pub fn from_source_input(input: &SourceBuildInputSnapshot) -> Result<Self, String> {
        Self::external(
            input.organization_id(),
            BuildSubject::external_source_revision(
                input.project_id(),
                input.environment_id(),
                input.source_revision_id(),
            ),
            input.repository().clone(),
            input.commit_sha().clone(),
            input.recipe().clone(),
            input.recipe_digest().as_str().to_owned(),
        )
    }

    pub fn external(
        organization_id: OrganizationId,
        subject: BuildSubject,
        repository: GitRepository,
        commit_sha: GitCommitSha,
        recipe: BuildRecipe,
        recipe_digest: String,
    ) -> Result<Self, String> {
        let canonical_repository = repository.canonical_url().to_owned();
        let source = Self {
            organization_id,
            subject,
            location: BuildSourceLocation::ExternalGit { repository },
            repository: canonical_repository,
            commit_sha,
            manifest_digest: None,
            recipe,
            recipe_digest,
        };
        source.validate()?;
        Ok(source)
    }

    pub fn hosted_asset(
        organization_id: OrganizationId,
        subject: BuildSubject,
        commit_sha: GitCommitSha,
        manifest_digest: Sha256Digest,
        recipe: BuildRecipe,
    ) -> Result<Self, String> {
        let asset_id = subject
            .asset_id()
            .ok_or_else(|| "hosted build source requires an Asset release subject".to_owned())?;
        let recipe_digest = recipe.digest()?;
        let source = Self {
            organization_id,
            subject,
            location: BuildSourceLocation::HostedAssetGit { asset_id },
            repository: format!("a3s://cloud/organizations/{organization_id}/assets/{asset_id}"),
            commit_sha,
            manifest_digest: Some(manifest_digest),
            recipe,
            recipe_digest,
        };
        source.validate()?;
        Ok(source)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.subject.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.repository.trim().is_empty()
            || self.repository.len() > 4096
            || self.repository.contains(['\0', '\r', '\n'])
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
            || self.recipe.clone().validate()? != self.recipe
            || self.recipe.digest()? != self.recipe_digest
        {
            return Err("resolved build source identity or recipe is invalid".into());
        }
        match (&self.location, &self.manifest_digest, self.subject) {
            (
                BuildSourceLocation::ExternalGit { repository },
                None,
                BuildSubject::ExternalSourceRevision { .. },
            ) if repository.canonical_url() == self.repository => Ok(()),
            (
                BuildSourceLocation::HostedAssetGit { asset_id },
                Some(manifest_digest),
                BuildSubject::AssetRelease {
                    asset_id: subject_asset_id,
                    ..
                },
            ) if *asset_id == subject_asset_id
                && Sha256Digest::parse(manifest_digest.as_str())? == *manifest_digest =>
            {
                Ok(())
            }
            _ => Err("resolved build source changed its typed source authority".into()),
        }
    }
}
