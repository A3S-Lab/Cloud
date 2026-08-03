use super::{git_directory, git_integrity, integrity, LocalAssetGitRepository};
use crate::modules::assets::domain::{
    Asset, AssetGitRepositoryError, AssetKind, AssetManifestAdmission, IAssetGitRepository,
};
use crate::modules::shared_kernel::domain::{GitCommitSha, Sha256Digest};
use crate::modules::sources::domain::BuildRecipe;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const ASSET_MANIFEST_PATH: &str = ".a3s/asset.acl";
const ASSET_MANIFEST_SCHEMA: &str = "a3s.cloud.asset.v1";
const MAX_ASSET_MANIFEST_BYTES: usize = 64 * 1024;

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
    if body.is_empty() || body.len() > MAX_ASSET_MANIFEST_BYTES {
        return Err(integrity("pinned Asset manifest has an invalid size"));
    }
    let source =
        std::str::from_utf8(&body).map_err(|_| integrity("pinned Asset manifest is not UTF-8"))?;
    let document = a3s_acl::parse(source)
        .map_err(|error| integrity(format!("pinned Asset manifest is invalid A3S ACL: {error}")))?;
    if document.blocks.len() != 1 {
        return Err(integrity(
            "pinned Asset manifest must contain exactly one asset block",
        ));
    }
    let block = &document.blocks[0];
    let keys = block
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if block.name != "asset"
        || !block.labels.is_empty()
        || block.blocks.len() > 1
        || keys != BTreeSet::from(["kind", "schema"])
    {
        return Err(integrity(
            "pinned Asset manifest has an unsupported structure",
        ));
    }
    let schema = block
        .attributes
        .get("schema")
        .and_then(a3s_acl::Value::as_str)
        .ok_or_else(|| integrity("pinned Asset manifest schema must be a string"))?;
    if schema != ASSET_MANIFEST_SCHEMA {
        return Err(integrity("pinned Asset manifest schema is unsupported"));
    }
    let kind = block
        .attributes
        .get("kind")
        .and_then(a3s_acl::Value::as_str)
        .ok_or_else(|| integrity("pinned Asset manifest kind must be a string"))?;
    let kind = AssetKind::parse(kind).map_err(integrity)?;
    if kind != asset.kind {
        return Err(integrity(
            "pinned Asset manifest kind does not match its Asset",
        ));
    }
    let build_recipe = block.blocks.first().map(parse_build_recipe).transpose()?;
    let admission = AssetManifestAdmission {
        commit_sha: commit_sha.clone(),
        manifest_digest: Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(&body)))
            .map_err(AssetGitRepositoryError::Integrity)?,
        kind,
        build_recipe,
    };
    admission
        .validate_for(asset.kind)
        .map_err(AssetGitRepositoryError::Integrity)?;
    Ok(admission)
}

fn parse_build_recipe(block: &a3s_acl::Block) -> Result<BuildRecipe, AssetGitRepositoryError> {
    let keys = block
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required = BTreeSet::from(["context", "file", "platforms"]);
    let allowed = BTreeSet::from(["context", "file", "platforms", "target"]);
    if block.name != "build"
        || !block.labels.is_empty()
        || !block.blocks.is_empty()
        || !required.is_subset(&keys)
        || !keys.is_subset(&allowed)
    {
        return Err(integrity(
            "pinned Asset build block has an unsupported structure",
        ));
    }
    let context = required_string(block, "context")?;
    let file = required_string(block, "file")?;
    let target = block
        .attributes
        .get("target")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| integrity("pinned Asset build target must be a string"))
        })
        .transpose()?;
    let platforms = match block.attributes.get("platforms") {
        Some(a3s_acl::Value::List(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| integrity("pinned Asset build platforms must be strings"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(integrity("pinned Asset build platforms must be a list")),
    };
    BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        context,
        file,
        target,
        platforms,
    )
    .map_err(|error| integrity(format!("pinned Asset build recipe is invalid: {error}")))
}

fn required_string<'a>(
    block: &'a a3s_acl::Block,
    field: &str,
) -> Result<&'a str, AssetGitRepositoryError> {
    block
        .attributes
        .get(field)
        .and_then(a3s_acl::Value::as_str)
        .ok_or_else(|| integrity(format!("pinned Asset build {field} must be a string")))
}
