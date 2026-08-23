use crate::modules::assets::domain::AssetKind;
use crate::modules::sources::domain::BuildRecipe;
use a3s_acl::{Block, Document, Value};
use std::collections::BTreeSet;

pub const ASSET_MANIFEST_PATH: &str = ".a3s/asset.acl";
pub const ASSET_MANIFEST_SCHEMA: &str = "a3s.cloud.asset.v1";
pub const ASSET_MANIFEST_MAX_ACL_BYTES: usize = 64 * 1024;

/// Assets-owned interpretation of the pinned `.a3s/asset.acl` contract.
///
/// Hosted Git admission and P0 source-layout detection both consume this one
/// parser so the Asset kind and optional build recipe cannot drift between
/// bounded contexts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetManifestDefinition {
    pub kind: AssetKind,
    pub build_recipe: Option<BuildRecipe>,
}

impl AssetManifestDefinition {
    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > ASSET_MANIFEST_MAX_ACL_BYTES {
            return Err("Asset manifest ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Asset manifest ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = a3s_acl::parse_acl(&normalized)
            .map_err(|error| format!("Asset manifest ACL is invalid: {error}"))?;
        parse_document(&document)
    }
}

fn parse_document(document: &Document) -> Result<AssetManifestDefinition, String> {
    if document.blocks.len() != 1 {
        return Err("Asset manifest must contain exactly one asset block".into());
    }
    let block = &document.blocks[0];
    let keys = attribute_keys(block);
    if block.name != "asset"
        || !block.labels.is_empty()
        || block.blocks.len() > 1
        || keys != BTreeSet::from(["kind", "schema"])
    {
        return Err("Asset manifest has an unsupported structure".into());
    }
    if required_string(block, "schema")? != ASSET_MANIFEST_SCHEMA {
        return Err("Asset manifest schema is unsupported".into());
    }
    let kind = AssetKind::parse(required_string(block, "kind")?)?;
    let build_recipe = block.blocks.first().map(parse_build_recipe).transpose()?;
    if kind == AssetKind::Skill && build_recipe.is_some() {
        return Err("Skill Asset manifest cannot contain a build recipe".into());
    }
    Ok(AssetManifestDefinition { kind, build_recipe })
}

fn parse_build_recipe(block: &Block) -> Result<BuildRecipe, String> {
    let keys = attribute_keys(block);
    let required = BTreeSet::from(["context", "file", "platforms"]);
    let allowed = BTreeSet::from(["context", "file", "platforms", "target"]);
    if block.name != "build"
        || !block.labels.is_empty()
        || !block.blocks.is_empty()
        || !required.is_subset(&keys)
        || !keys.is_subset(&allowed)
    {
        return Err("Asset manifest build block has an unsupported structure".into());
    }
    let target = block
        .attributes
        .get("target")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "Asset manifest build target must be a string".to_owned())
        })
        .transpose()?;
    let platforms = match block.attributes.get("platforms") {
        Some(Value::List(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "Asset manifest build platforms must be strings".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("Asset manifest build platforms must be a list".into()),
    };
    BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        required_string(block, "context")?,
        required_string(block, "file")?,
        target,
        platforms,
    )
    .map_err(|error| format!("Asset manifest build recipe is invalid: {error}"))
}

fn attribute_keys(block: &Block) -> BTreeSet<&str> {
    block.attributes.keys().map(String::as_str).collect()
}

fn required_string<'a>(block: &'a Block, field: &str) -> Result<&'a str, String> {
    block
        .attributes
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Asset manifest {field} must be a string"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shared_asset_build_contract() {
        let manifest = AssetManifestDefinition::parse_acl(concat!(
            "asset {\n",
            "  kind = \"agent\"\n",
            "  schema = \"a3s.cloud.asset.v1\"\n",
            "  build {\n",
            "    context = \".\"\n",
            "    file = \"Dockerfile\"\n",
            "    platforms = [\"linux/amd64\"]\n",
            "    target = \"release\"\n",
            "  }\n",
            "}\n",
        ))
        .expect("Asset manifest");
        assert_eq!(manifest.kind, AssetKind::Agent);
        let recipe = manifest.build_recipe.expect("build recipe");
        assert_eq!(recipe.context_path(), ".");
        assert_eq!(recipe.dockerfile_path(), "Dockerfile");
        assert_eq!(recipe.target(), Some("release"));
    }

    #[test]
    fn rejects_unknown_structure_and_skill_builds() {
        assert!(AssetManifestDefinition::parse_acl(
            "asset { schema = \"a3s.cloud.asset.v1\" kind = \"agent\" extra = true }\n"
        )
        .is_err());
        assert!(AssetManifestDefinition::parse_acl(concat!(
            "asset {\n",
            "  kind = \"skill\"\n",
            "  schema = \"a3s.cloud.asset.v1\"\n",
            "  build {\n",
            "    context = \".\"\n",
            "    file = \"Dockerfile\"\n",
            "    platforms = [\"linux/amd64\"]\n",
            "  }\n",
            "}\n",
        ))
        .is_err());
    }
}
