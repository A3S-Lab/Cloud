use super::BuildPlanProposal;
use crate::modules::shared_kernel::domain::{Sha256Digest, SourceRevisionId};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const BUILD_PLAN_SCHEMA: &str = "a3s.cloud.build-plan.v1";
pub const BUILD_PLAN_MAX_ACL_BYTES: usize = 64 * 1024;
const BUILD_PLAN_BLOCK: &str = "build_plan";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedBuildPlanContractSpec {
    pub source_revision_id: SourceRevisionId,
    pub proposal: BuildPlanProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedBuildPlanContract {
    spec: AcceptedBuildPlanContractSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl AcceptedBuildPlanContract {
    pub fn from_proposal(
        source_revision_id: SourceRevisionId,
        proposal: BuildPlanProposal,
    ) -> Result<Self, String> {
        if source_revision_id.as_uuid().is_nil() {
            return Err("accepted BuildPlan Source revision identity is invalid".into());
        }
        proposal.validate()?;
        let spec = AcceptedBuildPlanContractSpec {
            source_revision_id,
            proposal,
        };
        let document = contract_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > BUILD_PLAN_MAX_ACL_BYTES {
            return Err("accepted BuildPlan ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated accepted BuildPlan ACL is invalid: {error}"))?;
        let digest =
            Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
                format!("accepted BuildPlan ACL is not canonicalizable: {error}")
            })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > BUILD_PLAN_MAX_ACL_BYTES {
            return Err("accepted BuildPlan ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("accepted BuildPlan ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("accepted BuildPlan ACL is invalid: {error}"))?;
        let spec = parse_contract(&document)?;
        let value = Self::from_proposal(spec.source_revision_id, spec.proposal)?;
        if value.canonical_acl != normalized {
            return Err("accepted BuildPlan ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored accepted BuildPlan ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("accepted BuildPlan drifted from its canonical ACL".into());
        }
        Ok(())
    }

    pub const fn schema(&self) -> &'static str {
        BUILD_PLAN_SCHEMA
    }

    pub const fn spec(&self) -> &AcceptedBuildPlanContractSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &AcceptedBuildPlanContractSpec) -> Document {
    Document {
        blocks: vec![BlockBuilder::new(BUILD_PLAN_BLOCK)
            .attr("proposal_digest", string(spec.proposal.digest().as_str()))
            .attr("schema", string(BUILD_PLAN_SCHEMA))
            .attr(
                "source_revision_id",
                string(&spec.source_revision_id.to_string()),
            )
            .nested_block(spec.proposal.nested_block())
            .build()],
    }
}

fn parse_contract(document: &Document) -> Result<AcceptedBuildPlanContractSpec, String> {
    if document.blocks.len() != 1 {
        return Err("accepted BuildPlan must contain one top-level block".into());
    }
    let root = &document.blocks[0];
    let keys = root
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if root.name != BUILD_PLAN_BLOCK
        || !root.labels.is_empty()
        || keys != BTreeSet::from(["proposal_digest", "schema", "source_revision_id"])
        || root.blocks.len() != 1
        || root.blocks[0].name != "proposal"
    {
        return Err("accepted BuildPlan block shape is invalid".into());
    }
    if required_string(root, "schema")? != BUILD_PLAN_SCHEMA {
        return Err("accepted BuildPlan schema is unsupported".into());
    }
    let source_revision_id = Uuid::parse_str(&required_string(root, "source_revision_id")?)
        .map_err(|_| "accepted BuildPlan Source revision identity is invalid".to_owned())?;
    if source_revision_id.is_nil() {
        return Err("accepted BuildPlan Source revision identity is invalid".into());
    }
    let proposal = BuildPlanProposal::from_nested_block(&root.blocks[0])?;
    let proposal_digest = Sha256Digest::parse(required_string(root, "proposal_digest")?)?;
    if proposal_digest != *proposal.digest() {
        return Err("accepted BuildPlan proposal digest does not match its proposal".into());
    }
    Ok(AcceptedBuildPlanContractSpec {
        source_revision_id: SourceRevisionId::from_uuid(source_revision_id),
        proposal,
    })
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match block
        .attributes
        .get(name)
        .ok_or_else(|| format!("accepted BuildPlan field {name:?} is required"))?
    {
        Value::String(value) => Ok(value.clone()),
        _ => Err(format!(
            "accepted BuildPlan field {name:?} must be a string"
        )),
    }
}
