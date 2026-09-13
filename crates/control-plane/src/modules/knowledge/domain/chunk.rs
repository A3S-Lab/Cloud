use super::acl::{
    exact_shape, required_digest, required_string, required_string_list, required_u64,
    required_uuid, validate_non_nil, validate_tags,
};
use super::types::{
    KnowledgeChunkStructureV1, KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeChunkId, KnowledgeDocumentId, OrganizationId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{list, number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const BLOCK: &str = "knowledge_chunk";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeChunkSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub document_id: KnowledgeDocumentId,
    pub chunk_id: KnowledgeChunkId,
    pub structure: KnowledgeChunkStructureV1,
    pub ordinal: u64,
    pub parent_chunk_id: Option<KnowledgeChunkId>,
    pub content_digest: Sha256Digest,
    pub object_ref: String,
    pub tags: Vec<String>,
    pub provenance_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeChunkV1 {
    spec: KnowledgeChunkSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeChunkV1 {
    pub fn from_spec(spec: KnowledgeChunkSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.document_id.as_uuid(), "KnowledgeDocument")?;
        validate_non_nil(spec.chunk_id.as_uuid(), "KnowledgeChunk")?;
        if spec.object_ref.is_empty()
            || spec.object_ref.len() > 512
            || spec.object_ref.chars().any(char::is_control)
        {
            return Err("KnowledgeChunk object reference is invalid".into());
        }
        validate_tags(&spec.tags)?;
        match (spec.structure, spec.parent_chunk_id) {
            (KnowledgeChunkStructureV1::ParentChild, Some(parent)) => {
                validate_non_nil(parent.as_uuid(), "parent KnowledgeChunk")?;
                if parent == spec.chunk_id {
                    return Err("KnowledgeChunk cannot parent itself".into());
                }
            }
            (KnowledgeChunkStructureV1::ParentChild, None) => {}
            (_, Some(_)) => {
                return Err("only parent_child KnowledgeChunk may declare a parent".into());
            }
            _ => {}
        }
        Sha256Digest::parse(spec.content_digest.as_str())?;
        Sha256Digest::parse(spec.provenance_digest.as_str())?;
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr("chunk_id", string(&spec.chunk_id.to_string()))
                .attr("content_digest", string(spec.content_digest.as_str()))
                .attr("document_id", string(&spec.document_id.to_string()))
                .attr("object_ref", string(&spec.object_ref))
                .attr("ordinal", number(spec.ordinal as f64))
                .attr(
                    "organization_id",
                    string(&spec.organization_id.to_string()),
                )
                .attr(
                    "parent_chunk_id",
                    string(
                        &spec
                            .parent_chunk_id
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                    ),
                )
                .attr("project_id", string(&spec.project_id.to_string()))
                .attr(
                    "provenance_digest",
                    string(spec.provenance_digest.as_str()),
                )
                .attr("schema", string(KNOWLEDGE_CHUNK_SCHEMA_V1))
                .attr("structure", string(spec.structure.as_str()))
                .attr("tags", list(spec.tags.iter().map(|value| string(value.as_str())).collect()))
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
            return Err("stored KnowledgeChunk ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeChunkSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeChunkSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeChunk must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "chunk_id",
            "content_digest",
            "document_id",
            "object_ref",
            "ordinal",
            "organization_id",
            "parent_chunk_id",
            "project_id",
            "provenance_digest",
            "schema",
            "structure",
            "tags",
        ],
        &[],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_CHUNK_SCHEMA_V1 {
        return Err("KnowledgeChunk schema is unsupported".into());
    }
    let parent_raw = required_string(root, "parent_chunk_id")?;
    let parent_chunk_id = if parent_raw.is_empty() {
        None
    } else {
        let value = Uuid::parse_str(&parent_raw)
            .map_err(|_| "Knowledge parent_chunk_id must be a UUID".to_owned())?;
        validate_non_nil(value, "parent KnowledgeChunk")?;
        Some(KnowledgeChunkId::from_uuid(value))
    };
    Ok(KnowledgeChunkSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        document_id: KnowledgeDocumentId::from_uuid(required_uuid(root, "document_id")?),
        chunk_id: KnowledgeChunkId::from_uuid(required_uuid(root, "chunk_id")?),
        structure: KnowledgeChunkStructureV1::parse(&required_string(root, "structure")?)?,
        ordinal: required_u64(root, "ordinal", u64::MAX)?,
        parent_chunk_id,
        content_digest: required_digest(root, "content_digest")?,
        object_ref: required_string(root, "object_ref")?,
        tags: required_string_list(root, "tags")?,
        provenance_digest: required_digest(root, "provenance_digest")?,
    })
}

fn seal(spec: KnowledgeChunkSpecV1, document: Document) -> Result<KnowledgeChunkV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeChunk ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl)
        .map_err(|error| format!("generated KnowledgeChunk ACL is invalid: {error}"))?;
    let digest = Sha256Digest::parse(
        canonical_digest(&reparsed)
            .map_err(|error| format!("KnowledgeChunk contract is not canonicalizable: {error}"))?,
    )?;
    Ok(KnowledgeChunkV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeChunkV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeChunk ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeChunk ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeChunk ACL is invalid: {error}"))?;
    let value = KnowledgeChunkV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeChunk ACL is not canonical".into());
    }
    Ok(value)
}
