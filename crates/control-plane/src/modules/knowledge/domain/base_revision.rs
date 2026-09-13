use super::acl::{
    exact_shape, required_digest, required_string, required_string_list, required_timestamp,
    required_u64, required_uuid, validate_name, validate_non_nil, validate_tags,
};
use super::types::{
    KnowledgeChunkStructureV1, KNOWLEDGE_BASE_REVISION_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, KnowledgeBaseId, KnowledgeBaseRevisionId, OrganizationId, ProjectId,
    Sha256Digest,
};
use a3s_acl::builder::{list, number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_base_revision";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeBaseRevisionSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub revision_id: KnowledgeBaseRevisionId,
    pub generation: u64,
    pub name: String,
    pub chunk_structure: KnowledgeChunkStructureV1,
    pub retention_until: DateTime<Utc>,
    pub tags: Vec<String>,
    pub provenance_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeBaseRevisionV1 {
    spec: KnowledgeBaseRevisionSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeBaseRevisionV1 {
    pub fn from_spec(mut spec: KnowledgeBaseRevisionSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(spec.revision_id.as_uuid(), "KnowledgeBaseRevision")?;
        if spec.generation == 0 {
            return Err("KnowledgeBaseRevision generation must be positive".into());
        }
        validate_name(&spec.name)?;
        validate_tags(&spec.tags)?;
        spec.retention_until = canonical_timestamp(spec.retention_until);
        sealed(spec)
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        parse_sealed(source, parse_spec)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored KnowledgeBaseRevision ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeBaseRevisionSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn sealed(spec: KnowledgeBaseRevisionSpecV1) -> Result<KnowledgeBaseRevisionV1, String> {
    let document = Document {
        blocks: vec![BlockBuilder::new(BLOCK)
            .attr("chunk_structure", string(spec.chunk_structure.as_str()))
            .attr("generation", number(spec.generation as f64))
            .attr(
                "knowledge_base_id",
                string(&spec.knowledge_base_id.to_string()),
            )
            .attr("name", string(&spec.name))
            .attr(
                "organization_id",
                string(&spec.organization_id.to_string()),
            )
            .attr("project_id", string(&spec.project_id.to_string()))
            .attr(
                "provenance_digest",
                string(spec.provenance_digest.as_str()),
            )
            .attr(
                "retention_until",
                string(
                    &spec
                        .retention_until
                        .to_rfc3339_opts(SecondsFormat::Micros, true),
                ),
            )
            .attr("revision_id", string(&spec.revision_id.to_string()))
            .attr("schema", string(KNOWLEDGE_BASE_REVISION_SCHEMA_V1))
            .attr(
                "tags",
                list(spec.tags.iter().map(|value| string(value.as_str())).collect()),
            )
            .build()],
    };
    seal(spec, document)
}

fn parse_spec(document: &Document) -> Result<KnowledgeBaseRevisionSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeBaseRevision must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "chunk_structure",
            "generation",
            "knowledge_base_id",
            "name",
            "organization_id",
            "project_id",
            "provenance_digest",
            "retention_until",
            "revision_id",
            "schema",
            "tags",
        ],
        &[],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_BASE_REVISION_SCHEMA_V1 {
        return Err("KnowledgeBaseRevision schema is unsupported".into());
    }
    Ok(KnowledgeBaseRevisionSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(root, "revision_id")?),
        generation: required_u64(root, "generation", u64::MAX)?,
        name: required_string(root, "name")?,
        chunk_structure: KnowledgeChunkStructureV1::parse(&required_string(
            root,
            "chunk_structure",
        )?)?,
        retention_until: required_timestamp(root, "retention_until")?,
        tags: required_string_list(root, "tags")?,
        provenance_digest: required_digest(root, "provenance_digest")?,
    })
}

fn seal(
    spec: KnowledgeBaseRevisionSpecV1,
    document: Document,
) -> Result<KnowledgeBaseRevisionV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeBaseRevision ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl)
        .map_err(|error| format!("generated KnowledgeBaseRevision ACL is invalid: {error}"))?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeBaseRevision contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeBaseRevisionV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(
    source: &str,
    parse: impl FnOnce(&Document) -> Result<KnowledgeBaseRevisionSpecV1, String>,
) -> Result<KnowledgeBaseRevisionV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeBaseRevision ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeBaseRevision ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeBaseRevision ACL is invalid: {error}"))?;
    let value = KnowledgeBaseRevisionV1::from_spec(parse(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeBaseRevision ACL is not canonical".into());
    }
    Ok(value)
}
