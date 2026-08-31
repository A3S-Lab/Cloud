use super::{git_directory, git_integrity, integrity, LocalAssetGitRepository};
use crate::modules::assets::domain::{
    AgentReleaseTemplate, Asset, AssetGitRepositoryError, AssetKind, AssetManifestAdmission,
    AssetManifestDefinition, IAssetGitRepository, AGENT_RELEASE_TEMPLATE_MAX_ACL_BYTES,
    AGENT_RELEASE_TEMPLATE_PATH, ASSET_MANIFEST_MAX_ACL_BYTES, ASSET_MANIFEST_PATH,
};
use crate::modules::shared_kernel::domain::{GitCommitSha, Sha256Digest};
use sha2::{Digest, Sha256};

pub(super) async fn admit(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    commit_sha: &GitCommitSha,
) -> Result<AssetManifestAdmission, AssetGitRepositoryError> {
    store.inspect(asset).await?;
    GitCommitSha::parse(commit_sha.as_str()).map_err(AssetGitRepositoryError::Invalid)?;
    let repository = store.repository_path(asset);
    let reachable = store
        .git(vec![
            git_directory(&repository),
            "for-each-ref".into(),
            format!("--contains={commit_sha}").into(),
            "--format=%(refname)".into(),
            "--count=1".into(),
        ])
        .await
        .map_err(git_integrity("verify Asset manifest commit reachability"))?;
    if reachable.is_empty() {
        return Err(integrity(
            "Asset manifest commit is not reachable from an advertised reference",
        ));
    }
    let body = store
        .git(vec![
            git_directory(&repository),
            "show".into(),
            format!("{}:{ASSET_MANIFEST_PATH}", commit_sha.as_str()).into(),
        ])
        .await
        .map_err(git_integrity("read pinned Asset manifest"))?;
    if body.is_empty() || body.len() > ASSET_MANIFEST_MAX_ACL_BYTES {
        return Err(integrity("pinned Asset manifest has an invalid size"));
    }
    let source =
        std::str::from_utf8(&body).map_err(|_| integrity("pinned Asset manifest is not UTF-8"))?;
    let definition = AssetManifestDefinition::parse_acl(source)
        .map_err(|error| integrity(format!("pinned {error}")))?;
    let kind = definition.kind;
    if kind != asset.kind {
        return Err(integrity(
            "pinned Asset manifest kind does not match its Asset",
        ));
    }
    let agent_release_template = if kind == AssetKind::Agent {
        let body = store
            .git(vec![
                git_directory(&repository),
                "show".into(),
                format!("{}:{AGENT_RELEASE_TEMPLATE_PATH}", commit_sha.as_str()).into(),
            ])
            .await
            .map_err(git_integrity("read pinned Agent release template"))?;
        if body.is_empty() || body.len() > AGENT_RELEASE_TEMPLATE_MAX_ACL_BYTES {
            return Err(integrity(
                "pinned Agent release template has an invalid size",
            ));
        }
        let source = std::str::from_utf8(&body)
            .map_err(|_| integrity("pinned Agent release template is not UTF-8"))?;
        Some(
            AgentReleaseTemplate::parse(source)
                .map_err(|error| integrity(format!("pinned {error}")))?,
        )
    } else {
        None
    };
    let admission = AssetManifestAdmission {
        commit_sha: commit_sha.clone(),
        manifest_digest: Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(&body)))
            .map_err(AssetGitRepositoryError::Integrity)?,
        kind,
        build_recipe: definition.build_recipe,
        agent_release_template,
    };
    admission
        .validate_for(asset.kind)
        .map_err(AssetGitRepositoryError::Integrity)?;
    Ok(admission)
}
