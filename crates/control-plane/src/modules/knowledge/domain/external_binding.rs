use super::acl::{
    exact_shape, required_digest, required_string, required_uuid, validate_name, validate_non_nil,
};
use super::types::{EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES};
use crate::modules::shared_kernel::domain::{
    ExternalKnowledgeBindingId, KnowledgeBaseId, OrganizationId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "external_knowledge_binding";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalKnowledgeBindingSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub binding_id: ExternalKnowledgeBindingId,
    pub display_name: String,
    pub external_corpus_ref: String,
    pub external_corpus_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalKnowledgeBindingV1 {
    spec: ExternalKnowledgeBindingSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl ExternalKnowledgeBindingV1 {
    pub fn from_spec(spec: ExternalKnowledgeBindingSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(spec.binding_id.as_uuid(), "ExternalKnowledgeBinding")?;
        validate_name(&spec.display_name)?;
        if spec.external_corpus_ref.is_empty()
            || spec.external_corpus_ref.len() > 256
            || spec.external_corpus_ref.chars().any(char::is_control)
            || spec.external_corpus_ref.contains([' ', '/', '\\'])
        {
            return Err("external Knowledge corpus reference is invalid".into());
        }
        Sha256Digest::parse(spec.external_corpus_digest.as_str())?;
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr("binding_id", string(&spec.binding_id.to_string()))
                .attr("display_name", string(&spec.display_name))
                .attr(
                    "external_corpus_digest",
                    string(spec.external_corpus_digest.as_str()),
                )
                .attr("external_corpus_ref", string(&spec.external_corpus_ref))
                .attr(
                    "knowledge_base_id",
                    string(&spec.knowledge_base_id.to_string()),
                )
                .attr(
                    "organization_id",
                    string(&spec.organization_id.to_string()),
                )
                .attr("project_id", string(&spec.project_id.to_string()))
                .attr("schema", string(EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1))
                .build()],
        };
        seal(spec, document)
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        parse_sealed(source)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored ExternalKnowledgeBinding ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &ExternalKnowledgeBindingSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<ExternalKnowledgeBindingSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("ExternalKnowledgeBinding must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "binding_id",
            "display_name",
            "external_corpus_digest",
            "external_corpus_ref",
            "knowledge_base_id",
            "organization_id",
            "project_id",
            "schema",
        ],
        &[],
    )?;
    if required_string(root, "schema")? != EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1 {
        return Err("ExternalKnowledgeBinding schema is unsupported".into());
    }
    Ok(ExternalKnowledgeBindingSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        binding_id: ExternalKnowledgeBindingId::from_uuid(required_uuid(root, "binding_id")?),
        display_name: required_string(root, "display_name")?,
        external_corpus_ref: required_string(root, "external_corpus_ref")?,
        external_corpus_digest: required_digest(root, "external_corpus_digest")?,
    })
}

fn seal(
    spec: ExternalKnowledgeBindingSpecV1,
    document: Document,
) -> Result<ExternalKnowledgeBindingV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("ExternalKnowledgeBinding ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl)
        .map_err(|error| format!("generated ExternalKnowledgeBinding ACL is invalid: {error}"))?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("ExternalKnowledgeBinding contract is not canonicalizable: {error}")
    })?)?;
    Ok(ExternalKnowledgeBindingV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<ExternalKnowledgeBindingV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("ExternalKnowledgeBinding ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("ExternalKnowledgeBinding ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("ExternalKnowledgeBinding ACL is invalid: {error}"))?;
    let value = ExternalKnowledgeBindingV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("ExternalKnowledgeBinding ACL is not canonical".into());
    }
    Ok(value)
}
