use super::acl::{
    exact_child, exact_shape, required_digest, required_string, required_string_list,
    required_timestamp, required_u64, required_uuid, validate_name, validate_non_nil,
    validate_tags,
};
use super::types::{KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_DOCUMENT_SCHEMA_V1};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeDocumentId,
    OrganizationId, ProjectId, Sha256Digest, UserFileId,
};
use a3s_acl::builder::{list, number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_document";
const MAX_CONTENT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeContentReferenceV1 {
    pub object_ref: String,
    pub digest: Sha256Digest,
    pub size_bytes: u64,
    pub media_type: String,
}

impl KnowledgeContentReferenceV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.object_ref.is_empty()
            || self.object_ref.chars().any(char::is_control)
            || self.object_ref.len() > 512
        {
            return Err("Knowledge content object reference is invalid".into());
        }
        if self.size_bytes == 0 || self.size_bytes > MAX_CONTENT_BYTES {
            return Err("Knowledge content size must be bounded and positive".into());
        }
        if self.media_type.is_empty()
            || !self.media_type.is_ascii()
            || self.media_type.len() > 127
            || self.media_type.contains(['\0', '\r', '\n'])
            || !self.media_type.contains('/')
        {
            return Err("Knowledge content media type is invalid".into());
        }
        Sha256Digest::parse(self.digest.as_str())?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeDocumentSourceV1 {
    AdmittedUserFile {
        user_file_id: UserFileId,
        content_digest: Sha256Digest,
    },
    ImmutableObject {
        content: KnowledgeContentReferenceV1,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDocumentSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub document_id: KnowledgeDocumentId,
    pub title: String,
    pub retention_until: DateTime<Utc>,
    pub tags: Vec<String>,
    pub provenance_digest: Sha256Digest,
    pub source: KnowledgeDocumentSourceV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDocumentV1 {
    spec: KnowledgeDocumentSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeDocumentV1 {
    pub fn from_spec(mut spec: KnowledgeDocumentSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.document_id.as_uuid(), "KnowledgeDocument")?;
        validate_name(&spec.title)?;
        validate_tags(&spec.tags)?;
        match &spec.source {
            KnowledgeDocumentSourceV1::AdmittedUserFile {
                user_file_id,
                content_digest,
            } => {
                validate_non_nil(user_file_id.as_uuid(), "UserFile")?;
                Sha256Digest::parse(content_digest.as_str())?;
            }
            KnowledgeDocumentSourceV1::ImmutableObject { content } => content.validate()?,
        }
        spec.retention_until = canonical_timestamp(spec.retention_until);
        let document = contract_document(&spec);
        seal(spec, document)
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        parse_sealed(source)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored KnowledgeDocument ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeDocumentSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &KnowledgeDocumentSpecV1) -> Document {
    let source_block = match &spec.source {
        KnowledgeDocumentSourceV1::AdmittedUserFile {
            user_file_id,
            content_digest,
        } => BlockBuilder::new("source")
            .attr("kind", string("admitted_user_file"))
            .attr("content_digest", string(content_digest.as_str()))
            .attr("user_file_id", string(&user_file_id.to_string()))
            .build(),
        KnowledgeDocumentSourceV1::ImmutableObject { content } => BlockBuilder::new("source")
            .attr("kind", string("immutable_object"))
            .attr("content_digest", string(content.digest.as_str()))
            .attr("media_type", string(&content.media_type))
            .attr("object_ref", string(&content.object_ref))
            .attr("size_bytes", number(content.size_bytes as f64))
            .build(),
    };
    Document {
        blocks: vec![BlockBuilder::new(BLOCK)
            .attr("document_id", string(&spec.document_id.to_string()))
            .attr(
                "knowledge_base_id",
                string(&spec.knowledge_base_id.to_string()),
            )
            .attr(
                "knowledge_base_revision_id",
                string(&spec.knowledge_base_revision_id.to_string()),
            )
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
            .attr("schema", string(KNOWLEDGE_DOCUMENT_SCHEMA_V1))
            .attr("tags", list(spec.tags.iter().map(|value| string(value.as_str())).collect()))
            .attr("title", string(&spec.title))
            .nested_block(source_block)
            .build()],
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeDocumentSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeDocument must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "document_id",
            "knowledge_base_id",
            "knowledge_base_revision_id",
            "organization_id",
            "project_id",
            "provenance_digest",
            "retention_until",
            "schema",
            "tags",
            "title",
        ],
        &["source"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_DOCUMENT_SCHEMA_V1 {
        return Err("KnowledgeDocument schema is unsupported".into());
    }
    let source = exact_child(root, "source")?;
    let kind = required_string(source, "kind")?;
    let source = match kind.as_str() {
        "admitted_user_file" => {
            exact_shape(
                source,
                "source",
                &["content_digest", "kind", "user_file_id"],
                &[],
            )?;
            KnowledgeDocumentSourceV1::AdmittedUserFile {
                user_file_id: UserFileId::from_uuid(required_uuid(source, "user_file_id")?),
                content_digest: required_digest(source, "content_digest")?,
            }
        }
        "immutable_object" => {
            exact_shape(
                source,
                "source",
                &["content_digest", "kind", "media_type", "object_ref", "size_bytes"],
                &[],
            )?;
            KnowledgeDocumentSourceV1::ImmutableObject {
                content: KnowledgeContentReferenceV1 {
                    object_ref: required_string(source, "object_ref")?,
                    digest: required_digest(source, "content_digest")?,
                    size_bytes: required_u64(source, "size_bytes", MAX_CONTENT_BYTES)?,
                    media_type: required_string(source, "media_type")?,
                },
            }
        }
        _ => return Err("KnowledgeDocument source kind is unsupported".into()),
    };
    Ok(KnowledgeDocumentSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        document_id: KnowledgeDocumentId::from_uuid(required_uuid(root, "document_id")?),
        title: required_string(root, "title")?,
        retention_until: required_timestamp(root, "retention_until")?,
        tags: required_string_list(root, "tags")?,
        provenance_digest: required_digest(root, "provenance_digest")?,
        source,
    })
}

fn seal(spec: KnowledgeDocumentSpecV1, document: Document) -> Result<KnowledgeDocumentV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeDocument ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl)
        .map_err(|error| format!("generated KnowledgeDocument ACL is invalid: {error}"))?;
    let digest = Sha256Digest::parse(
        canonical_digest(&reparsed)
            .map_err(|error| format!("KnowledgeDocument contract is not canonicalizable: {error}"))?,
    )?;
    Ok(KnowledgeDocumentV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeDocumentV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeDocument ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeDocument ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeDocument ACL is invalid: {error}"))?;
    let value = KnowledgeDocumentV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeDocument ACL is not canonical".into());
    }
    Ok(value)
}
