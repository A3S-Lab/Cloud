use crate::modules::assets::domain::{
    Asset, AssetGitRepositoryError, AssetKind, AssetManifestAdmission, AssetRelease,
    IAssetGitRepository, IAssetRepository,
};
use crate::modules::assets::published::{
    HostedAgentReleaseTemplate, HostedAssetBuildInputSnapshot,
    ValidatedHostedAssetBuildInputProjection,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, OrganizationId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostedAssetBuildInputQueryError {
    #[error("hosted Asset build input request is invalid: {0}")]
    Invalid(String),
    #[error("hosted Asset build input identity conflicts with durable state")]
    Conflict,
    #[error("hosted Asset build input was not found")]
    NotFound,
    #[error("hosted Asset build input is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("hosted Asset build input failed integrity validation: {0}")]
    Integrity(String),
    #[error("hosted Asset build input storage failed: {0}")]
    Storage(String),
}

/// Assets-owned query boundary for one exact hosted build input.
///
/// Consumers provide the complete tenant/release identity and receive either
/// one immutable published snapshot or absence. Asset aggregates, release
/// lifecycle, Git repositories, and persistence errors remain inside Assets.
#[async_trait]
pub trait IHostedAssetBuildInputQueryPort: Send + Sync {
    async fn find_hosted_asset_build_input(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<HostedAssetBuildInputSnapshot>, HostedAssetBuildInputQueryError>;
}

pub struct HostedAssetBuildInputQueryService {
    assets: Arc<dyn IAssetRepository>,
    repositories: Arc<dyn IAssetGitRepository>,
}

impl HostedAssetBuildInputQueryService {
    pub fn new(
        assets: Arc<dyn IAssetRepository>,
        repositories: Arc<dyn IAssetGitRepository>,
    ) -> Self {
        Self {
            assets,
            repositories,
        }
    }
}

#[async_trait]
impl IHostedAssetBuildInputQueryPort for HostedAssetBuildInputQueryService {
    async fn find_hosted_asset_build_input(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<HostedAssetBuildInputSnapshot>, HostedAssetBuildInputQueryError> {
        if organization_id.as_uuid().is_nil()
            || asset_id.as_uuid().is_nil()
            || asset_release_id.as_uuid().is_nil()
        {
            return Err(HostedAssetBuildInputQueryError::Invalid(
                "hosted Asset build input identity cannot contain nil IDs".into(),
            ));
        }
        let asset = match self.assets.find_asset(organization_id, asset_id).await {
            Ok(Some(asset)) => asset,
            Ok(None) | Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(map_repository_error(error)),
        };
        let release = match self
            .assets
            .find_release(organization_id, asset_id, asset_release_id)
            .await
        {
            Ok(Some(release)) => release,
            Ok(None) | Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(map_repository_error(error)),
        };
        if asset.organization_id != organization_id
            || asset.id != asset_id
            || release.organization_id != organization_id
            || release.asset_id != asset_id
            || release.id != asset_release_id
        {
            return Err(HostedAssetBuildInputQueryError::Conflict);
        }
        let release = admit_hosted_build_release(&asset, &release)?;
        let admission = self
            .repositories
            .admit_manifest(&asset, &release.commit_sha)
            .await
            .map_err(map_asset_git_error)?;
        project_hosted_build_input(release, admission).map(Some)
    }
}

struct AdmittedHostedAssetRelease {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    kind: AssetKind,
    commit_sha: GitCommitSha,
    manifest_digest: Sha256Digest,
}

fn admit_hosted_build_release(
    asset: &Asset,
    release: &AssetRelease,
) -> Result<AdmittedHostedAssetRelease, HostedAssetBuildInputQueryError> {
    release
        .validate_for(asset)
        .map_err(HostedAssetBuildInputQueryError::Integrity)?;
    if asset.kind == AssetKind::Skill {
        return Err(HostedAssetBuildInputQueryError::Invalid(
            "Skill bundle publication is owned by A0.5 and cannot use the OCI build output contract"
                .into(),
        ));
    }
    Ok(AdmittedHostedAssetRelease {
        organization_id: release.organization_id,
        asset_id: release.asset_id,
        asset_release_id: release.id,
        kind: asset.kind,
        commit_sha: release.commit_sha.clone(),
        manifest_digest: release.manifest_digest.clone(),
    })
}

fn project_hosted_build_input(
    release: AdmittedHostedAssetRelease,
    admission: AssetManifestAdmission,
) -> Result<HostedAssetBuildInputSnapshot, HostedAssetBuildInputQueryError> {
    admission
        .validate_for(release.kind)
        .map_err(HostedAssetBuildInputQueryError::Integrity)?;
    if admission.commit_sha != release.commit_sha
        || admission.manifest_digest != release.manifest_digest
    {
        return Err(HostedAssetBuildInputQueryError::Integrity(
            "pinned Asset manifest changed after release draft creation".into(),
        ));
    }
    let recipe = admission.build_recipe.ok_or_else(|| {
        HostedAssetBuildInputQueryError::Invalid(
            "Agent and MCP release publication requires one pinned Asset build block".into(),
        )
    })?;
    let agent_release_template = admission
        .agent_release_template
        .map(|template| {
            HostedAgentReleaseTemplate::from_validated_parts(
                template.identity().into(),
                template.canonical_acl().into(),
            )
        })
        .transpose()
        .map_err(HostedAssetBuildInputQueryError::Integrity)?;
    HostedAssetBuildInputSnapshot::from_validated_release(
        ValidatedHostedAssetBuildInputProjection {
            organization_id: release.organization_id,
            asset_id: release.asset_id,
            asset_release_id: release.asset_release_id,
            commit_sha: release.commit_sha,
            manifest_digest: release.manifest_digest,
            recipe,
            agent_release_template,
        },
    )
    .map_err(HostedAssetBuildInputQueryError::Integrity)
}

fn map_repository_error(error: RepositoryError) -> HostedAssetBuildInputQueryError {
    match error {
        RepositoryError::NotFound => HostedAssetBuildInputQueryError::Storage(
            "Asset repository returned an unexpected not-found error".into(),
        ),
        RepositoryError::Conflict(_)
        | RepositoryError::Forbidden(_)
        | RepositoryError::IdempotencyConflict => HostedAssetBuildInputQueryError::Conflict,
        RepositoryError::Storage(message) => HostedAssetBuildInputQueryError::Storage(message),
    }
}

fn map_asset_git_error(error: AssetGitRepositoryError) -> HostedAssetBuildInputQueryError {
    match error {
        AssetGitRepositoryError::Invalid(message) => {
            HostedAssetBuildInputQueryError::Invalid(message)
        }
        AssetGitRepositoryError::NotFound => HostedAssetBuildInputQueryError::NotFound,
        AssetGitRepositoryError::Integrity(message) => {
            HostedAssetBuildInputQueryError::Integrity(message)
        }
        AssetGitRepositoryError::QuotaExceeded => HostedAssetBuildInputQueryError::Invalid(
            "hosted Git repository quota was exceeded".into(),
        ),
        AssetGitRepositoryError::BackupUnavailable => HostedAssetBuildInputQueryError::Unavailable(
            "hosted Git repository is unavailable".into(),
        ),
        AssetGitRepositoryError::Storage(message) => {
            HostedAssetBuildInputQueryError::Storage(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::assets::domain::{AssetReleaseVersion, AssetState};
    use crate::modules::shared_kernel::domain::{GitCommitSha, ResourceName, Sha256Digest};
    use crate::modules::sources::published::BuildRecipe;
    use chrono::Utc;

    #[test]
    fn projects_only_the_exact_pinned_agent_input() {
        let (asset, release, admission) = fixture(AssetKind::Agent);
        let admitted = admit_hosted_build_release(&asset, &release).expect("admitted release");

        let input =
            project_hosted_build_input(admitted, admission).expect("hosted Asset build input");

        assert_eq!(input.schema(), HostedAssetBuildInputSnapshot::SCHEMA);
        assert_eq!(input.organization_id(), asset.organization_id);
        assert_eq!(input.asset_id(), asset.id);
        assert_eq!(input.asset_release_id(), release.id);
        assert_eq!(input.commit_sha(), &release.commit_sha);
        assert_eq!(input.manifest_digest(), &release.manifest_digest);
    }

    #[test]
    fn rejects_manifest_drift_and_skill_build_authority() {
        let (asset, release, mut admission) = fixture(AssetKind::Agent);
        admission.manifest_digest = digest('c');
        let admitted = admit_hosted_build_release(&asset, &release).expect("admitted release");
        assert!(matches!(
            project_hosted_build_input(admitted, admission),
            Err(HostedAssetBuildInputQueryError::Integrity(message))
                if message == "pinned Asset manifest changed after release draft creation"
        ));

        let (skill, release, _admission) = fixture(AssetKind::Skill);
        assert_eq!(skill.state, AssetState::Active);
        assert!(matches!(
            admit_hosted_build_release(&skill, &release),
            Err(HostedAssetBuildInputQueryError::Invalid(message))
                if message.contains("Skill bundle publication")
        ));
    }

    fn fixture(kind: AssetKind) -> (Asset, AssetRelease, AssetManifestAdmission) {
        let asset = Asset::create(
            AssetId::new(),
            OrganizationId::new(),
            ResourceName::parse("Hosted Input").expect("name"),
            kind,
            Utc::now(),
        )
        .expect("Asset");
        let commit_sha = GitCommitSha::parse("a".repeat(40)).expect("commit");
        let manifest_digest = digest('b');
        let release = AssetRelease::draft(
            &asset,
            AssetReleaseId::new(),
            AssetReleaseVersion::parse("1.0.0").expect("version"),
            commit_sha.clone(),
            manifest_digest.clone(),
            Utc::now(),
        )
        .expect("release");
        let build_recipe = if kind == AssetKind::Skill {
            None
        } else {
            Some(recipe())
        };
        let admission = AssetManifestAdmission {
            commit_sha,
            manifest_digest,
            kind,
            build_recipe,
            agent_release_template: (kind == AssetKind::Agent).then(|| agent_release_template()),
        };
        (asset, release, admission)
    }

    fn recipe() -> BuildRecipe {
        BuildRecipe::dockerfile(
            BuildRecipe::SCHEMA,
            BuildRecipe::DOCKERFILE_KIND,
            ".",
            "Dockerfile",
            None,
            vec!["linux/amd64".into()],
        )
        .expect("recipe")
    }

    fn agent_release_template() -> crate::modules::assets::domain::AgentReleaseTemplate {
        crate::modules::assets::domain::AgentReleaseTemplate::parse(concat!(
            "agent_release {\n",
            "  schema = \"a3s.code.agent-release.v1\"\n",
            "  protocol = \"a3s.code.agent.v1\"\n",
            "  artifact { digest = \"sha256:1111111111111111111111111111111111111111111111111111111111111111\" media_type = \"application/vnd.oci.image.manifest.v1+json\" }\n",
            "  entrypoint { command = \"/usr/bin/a3s\" args = [\"code\", \"harness\", \"--manifest\", \"/app/.a3s/asset.acl\"] }\n",
            "  health { transport = \"http\" port = 8080 readiness_path = \"/health/ready\" liveness_path = \"/health/live\" shutdown_grace_seconds = 30 }\n",
            "  storage { workspace = \"ephemeral\" cache = \"ephemeral\" persistent_data = \"none\" }\n",
            "  capability \"runtime.service\" { level = 1 }\n",
            "  capability \"secrets.external\" { level = 1 }\n",
            "  capability \"workspace.local\" { level = 1 }\n",
            "  provenance \"source\" { uri = \"urn:a3s:source:template\" digest = \"sha256:2222222222222222222222222222222222222222222222222222222222222222\" }\n",
            "  provenance \"builder\" { uri = \"urn:a3s:builder:template\" digest = \"sha256:4444444444444444444444444444444444444444444444444444444444444444\" }\n",
            "}\n",
        ))
        .expect("Agent release template")
    }

    fn digest(fill: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", fill.to_string().repeat(64))).expect("digest")
    }
}
