use super::source_layout::{
    path_is_within_root, validate_repository_file_path, validate_repository_root,
    SourceLayoutIdentity,
};
use crate::modules::shared_kernel::domain::{GitCommitSha, Sha256Digest};
use crate::modules::sources::domain::BuildRecipe;
use a3s_acl::builder::{list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;

pub const BUILD_PLAN_PROPOSAL_SCHEMA: &str = "a3s.cloud.build-plan-proposal.v1";
pub const BUILD_PLAN_DETECTOR_REVISION: &str = "p0.1-c1";
pub const BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES: usize = 64 * 1024;
const BUILD_PLAN_BLOCK: &str = "build_plan";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildPlanDetectorKind {
    AssetAcl,
    Dockerfile,
}

impl BuildPlanDetectorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AssetAcl => "asset_acl",
            Self::Dockerfile => "dockerfile",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "asset_acl" => Ok(Self::AssetAcl),
            "dockerfile" => Ok(Self::Dockerfile),
            _ => Err("BuildPlan detector kind is unsupported".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPlanProposalSpec {
    pub source: SourceLayoutIdentity,
    pub detector: BuildPlanDetectorKind,
    pub detector_revision: String,
    pub project_root: String,
    pub evidence_path: String,
    pub evidence_digest: Sha256Digest,
    pub recipe: BuildRecipe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPlanProposal {
    spec: BuildPlanProposalSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl BuildPlanProposal {
    pub fn from_spec(mut spec: BuildPlanProposalSpec) -> Result<Self, String> {
        spec.source.validate()?;
        if spec.detector_revision != BUILD_PLAN_DETECTOR_REVISION {
            return Err("BuildPlan detector revision is unsupported".into());
        }
        validate_repository_root(&spec.project_root)?;
        validate_repository_file_path(&spec.evidence_path)?;
        if Sha256Digest::parse(spec.evidence_digest.as_str())? != spec.evidence_digest {
            return Err("BuildPlan evidence digest is not canonical".into());
        }
        spec.recipe = spec.recipe.validate()?;
        if spec.project_root != spec.recipe.context_path()
            || !path_is_within_root(spec.recipe.dockerfile_path(), &spec.project_root)
        {
            return Err("BuildPlan project root does not contain its build recipe".into());
        }
        match spec.detector {
            BuildPlanDetectorKind::AssetAcl => {
                if spec.evidence_path != crate::modules::assets::domain::ASSET_MANIFEST_PATH {
                    return Err("Asset ACL BuildPlan evidence path is invalid".into());
                }
            }
            BuildPlanDetectorKind::Dockerfile => {
                if spec.evidence_path != spec.recipe.dockerfile_path() {
                    return Err("Dockerfile BuildPlan evidence does not match its recipe".into());
                }
            }
        }
        let document = proposal_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES {
            return Err("BuildPlan proposal ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated BuildPlan proposal ACL is invalid: {error}"))?;
        let digest =
            Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
                format!("BuildPlan proposal ACL is not canonicalizable: {error}")
            })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES {
            return Err("BuildPlan proposal ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("BuildPlan proposal ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("BuildPlan proposal ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_proposal(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("BuildPlan proposal ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored BuildPlan proposal ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("BuildPlan proposal drifted from its canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &BuildPlanProposalSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub(crate) fn canonical_cmp(&self, other: &Self) -> Ordering {
        self.spec
            .project_root
            .cmp(&other.spec.project_root)
            .then_with(|| self.spec.detector.cmp(&other.spec.detector))
            .then_with(|| self.digest.cmp(&other.digest))
    }

    pub(crate) fn nested_block(&self) -> Block {
        let mut block = proposal_document(&self.spec)
            .blocks
            .into_iter()
            .next()
            .expect("BuildPlan proposal document always has one root block");
        block.name = "proposal".into();
        block
    }

    pub(crate) fn from_nested_block(block: &Block) -> Result<Self, String> {
        if block.name != "proposal" {
            return Err("accepted BuildPlan proposal block is invalid".into());
        }
        let mut root = block.clone();
        root.name = BUILD_PLAN_BLOCK.into();
        Self::from_spec(parse_proposal(&Document { blocks: vec![root] })?)
    }
}

fn proposal_document(spec: &BuildPlanProposalSpec) -> Document {
    let source = BlockBuilder::new("source")
        .attr("commit_sha", string(spec.source.commit_sha.as_str()))
        .attr(
            "content_digest",
            string(spec.source.content_digest.as_str()),
        )
        .attr(
            "identity_digest",
            string(spec.source.source_identity_digest.as_str()),
        )
        .build();
    let evidence = BlockBuilder::new("evidence")
        .attr("content_digest", string(spec.evidence_digest.as_str()))
        .attr("path", string(&spec.evidence_path))
        .build();
    let mut build = BlockBuilder::new("build")
        .attr("context", string(spec.recipe.context_path()))
        .attr("file", string(spec.recipe.dockerfile_path()))
        .attr("kind", string(spec.recipe.kind()))
        .attr(
            "platforms",
            list(
                spec.recipe
                    .platforms()
                    .iter()
                    .map(|platform| string(platform.as_str()))
                    .collect(),
            ),
        )
        .attr("schema", string(spec.recipe.schema()));
    if let Some(target) = spec.recipe.target() {
        build = build.attr("target", string(target));
    }
    Document {
        blocks: vec![BlockBuilder::new(BUILD_PLAN_BLOCK)
            .attr("detector", string(spec.detector.as_str()))
            .attr("detector_revision", string(&spec.detector_revision))
            .attr("project_root", string(&spec.project_root))
            .attr("schema", string(BUILD_PLAN_PROPOSAL_SCHEMA))
            .nested_block(source)
            .nested_block(evidence)
            .nested_block(build.build())
            .build()],
    }
}

fn parse_proposal(document: &Document) -> Result<BuildPlanProposalSpec, String> {
    if document.blocks.len() != 1 {
        return Err("BuildPlan proposal must contain one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BUILD_PLAN_BLOCK,
        &["detector", "detector_revision", "project_root", "schema"],
        &["source", "evidence", "build"],
    )?;
    if required_string(root, "schema")? != BUILD_PLAN_PROPOSAL_SCHEMA {
        return Err("BuildPlan proposal schema is unsupported".into());
    }
    let source = exact_child(root, "source")?;
    exact_shape(
        source,
        "source",
        &["commit_sha", "content_digest", "identity_digest"],
        &[],
    )?;
    let evidence = exact_child(root, "evidence")?;
    exact_shape(evidence, "evidence", &["content_digest", "path"], &[])?;
    let build = exact_child(root, "build")?;
    let build_keys = build
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let build_required = BTreeSet::from(["context", "file", "kind", "platforms", "schema"]);
    let build_allowed =
        BTreeSet::from(["context", "file", "kind", "platforms", "schema", "target"]);
    if build.name != "build"
        || !build.labels.is_empty()
        || !build.blocks.is_empty()
        || !build_required.is_subset(&build_keys)
        || !build_keys.is_subset(&build_allowed)
    {
        return Err("BuildPlan proposal build block shape is invalid".into());
    }
    let recipe = BuildRecipe::dockerfile(
        &required_string(build, "schema")?,
        &required_string(build, "kind")?,
        &required_string(build, "context")?,
        &required_string(build, "file")?,
        optional_string(build, "target")?.as_deref(),
        required_string_list(build, "platforms")?,
    )?;
    Ok(BuildPlanProposalSpec {
        source: SourceLayoutIdentity::new(
            Sha256Digest::parse(required_string(source, "identity_digest")?)?,
            GitCommitSha::parse(required_string(source, "commit_sha")?)?,
            Sha256Digest::parse(required_string(source, "content_digest")?)?,
        )?,
        detector: BuildPlanDetectorKind::parse(&required_string(root, "detector")?)?,
        detector_revision: required_string(root, "detector_revision")?,
        project_root: required_string(root, "project_root")?,
        evidence_path: required_string(evidence, "path")?,
        evidence_digest: Sha256Digest::parse(required_string(evidence, "content_digest")?)?,
        recipe,
    })
}

fn exact_shape(
    block: &Block,
    name: &str,
    attributes: &[&str],
    children: &[&str],
) -> Result<(), String> {
    if block.name != name
        || !block.labels.is_empty()
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || block.blocks.len() != children.len()
        || block
            .blocks
            .iter()
            .any(|child| !children.contains(&child.name.as_str()))
    {
        return Err(format!("BuildPlan proposal {name} block shape is invalid"));
    }
    Ok(())
}

fn exact_child<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("BuildPlan proposal {name} block is required"))?;
    if matches.next().is_some() {
        return Err(format!("BuildPlan proposal {name} block must be unique"));
    }
    Ok(value)
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("BuildPlan proposal field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match required_value(block, name)? {
        Value::String(value) => Ok(value.clone()),
        _ => Err(format!(
            "BuildPlan proposal field {name:?} must be a string"
        )),
    }
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| match value {
            Value::String(value) => Ok(value.clone()),
            _ => Err(format!(
                "BuildPlan proposal field {name:?} must be a string"
            )),
        })
        .transpose()
}

fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    match required_value(block, name)? {
        Value::List(values) => values
            .iter()
            .map(|value| match value {
                Value::String(value) => Ok(value.clone()),
                _ => Err(format!(
                    "BuildPlan proposal field {name:?} must contain only strings"
                )),
            })
            .collect(),
        _ => Err(format!("BuildPlan proposal field {name:?} must be a list")),
    }
}
