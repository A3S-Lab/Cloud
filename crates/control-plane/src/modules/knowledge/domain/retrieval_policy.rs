use super::acl::{
    exact_shape, required_string, required_u32, required_uuid, validate_non_nil,
};
use super::types::{
    KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_MAX_TOP_K,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseRevisionId, KnowledgeRetrievalPolicyRevisionId, OrganizationId, ProjectId,
    Sha256Digest,
};
use a3s_acl::builder::{number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_retrieval_policy_revision";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRetrievalPolicyRevisionSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub policy_revision_id: KnowledgeRetrievalPolicyRevisionId,
    pub search_mode: String,
    pub filter_mode: String,
    pub rerank_mode: String,
    pub score_mode: String,
    pub citation_mode: String,
    pub top_k: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRetrievalPolicyRevisionV1 {
    spec: KnowledgeRetrievalPolicyRevisionSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeRetrievalPolicyRevisionV1 {
    pub fn from_spec(spec: KnowledgeRetrievalPolicyRevisionSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(
            spec.policy_revision_id.as_uuid(),
            "KnowledgeRetrievalPolicyRevision",
        )?;
        for (value, label) in [
            (&spec.search_mode, "search_mode"),
            (&spec.filter_mode, "filter_mode"),
            (&spec.rerank_mode, "rerank_mode"),
            (&spec.score_mode, "score_mode"),
            (&spec.citation_mode, "citation_mode"),
        ] {
            validate_closed_token(value, label)?;
        }
        if !(1..=KNOWLEDGE_MAX_TOP_K).contains(&spec.top_k) {
            return Err("Knowledge retrieval top_k is out of bounds".into());
        }
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr("citation_mode", string(&spec.citation_mode))
                .attr("filter_mode", string(&spec.filter_mode))
                .attr(
                    "knowledge_base_revision_id",
                    string(&spec.knowledge_base_revision_id.to_string()),
                )
                .attr(
                    "organization_id",
                    string(&spec.organization_id.to_string()),
                )
                .attr(
                    "policy_revision_id",
                    string(&spec.policy_revision_id.to_string()),
                )
                .attr("project_id", string(&spec.project_id.to_string()))
                .attr("rerank_mode", string(&spec.rerank_mode))
                .attr("schema", string(KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1))
                .attr("score_mode", string(&spec.score_mode))
                .attr("search_mode", string(&spec.search_mode))
                .attr("top_k", number(spec.top_k as f64))
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
            return Err(
                "stored KnowledgeRetrievalPolicyRevision ACL and digest do not match".into(),
            );
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeRetrievalPolicyRevisionSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_closed_token(value: &str, label: &str) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "disabled",
        "exact",
        "hybrid",
        "required",
        "semantic",
        "sparse",
        "standard",
    ];
    if !ALLOWED.contains(&value) {
        return Err(format!("Knowledge retrieval {label} is unsupported"));
    }
    Ok(())
}

fn parse_spec(document: &Document) -> Result<KnowledgeRetrievalPolicyRevisionSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err(
            "KnowledgeRetrievalPolicyRevision must contain exactly one top-level block".into(),
        );
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "citation_mode",
            "filter_mode",
            "knowledge_base_revision_id",
            "organization_id",
            "policy_revision_id",
            "project_id",
            "rerank_mode",
            "schema",
            "score_mode",
            "search_mode",
            "top_k",
        ],
        &[],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1 {
        return Err("KnowledgeRetrievalPolicyRevision schema is unsupported".into());
    }
    Ok(KnowledgeRetrievalPolicyRevisionSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        policy_revision_id: KnowledgeRetrievalPolicyRevisionId::from_uuid(required_uuid(
            root,
            "policy_revision_id",
        )?),
        search_mode: required_string(root, "search_mode")?,
        filter_mode: required_string(root, "filter_mode")?,
        rerank_mode: required_string(root, "rerank_mode")?,
        score_mode: required_string(root, "score_mode")?,
        citation_mode: required_string(root, "citation_mode")?,
        top_k: required_u32(root, "top_k", 1, KNOWLEDGE_MAX_TOP_K)?,
    })
}

fn seal(
    spec: KnowledgeRetrievalPolicyRevisionSpecV1,
    document: Document,
) -> Result<KnowledgeRetrievalPolicyRevisionV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeRetrievalPolicyRevision ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeRetrievalPolicyRevision ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeRetrievalPolicyRevision contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeRetrievalPolicyRevisionV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeRetrievalPolicyRevisionV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeRetrievalPolicyRevision ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeRetrievalPolicyRevision ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeRetrievalPolicyRevision ACL is invalid: {error}"))?;
    let value = KnowledgeRetrievalPolicyRevisionV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeRetrievalPolicyRevision ACL is not canonical".into());
    }
    Ok(value)
}
