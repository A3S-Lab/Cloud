use super::acl::{
    exact_child, exact_shape, required_digest, required_string, required_u64, required_uuid,
    validate_name, validate_non_nil,
};
use super::document::KnowledgeContentReferenceV1;
use super::types::{KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_DATASOURCE_ENTRANCE_SCHEMA_V1};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeDatasourceEntranceId, OrganizationId,
    ProjectId, Sha256Digest, UserFileId,
};
use a3s_acl::builder::{number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_datasource_entrance";
const MAX_CONTENT_BYTES: u64 = 512 * 1024 * 1024;

/// File-upload or inline-text datasource entrance for Knowledge ingestion (K0.2).
///
/// Digests of sealed entrances are what `KnowledgePipelineRelease` pins in
/// `datasource_entrance_digests`. Drive/web-crawl/marketplace kinds are refused
/// here until their owning gates supply real source contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeDatasourceEntranceKindV1 {
    FileUpload {
        user_file_id: UserFileId,
        content_digest: Sha256Digest,
    },
    InlineText {
        content: KnowledgeContentReferenceV1,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDatasourceEntranceSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub entrance_id: KnowledgeDatasourceEntranceId,
    pub name: String,
    pub provenance_digest: Sha256Digest,
    pub kind: KnowledgeDatasourceEntranceKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDatasourceEntranceV1 {
    spec: KnowledgeDatasourceEntranceSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeDatasourceEntranceV1 {
    pub fn from_spec(spec: KnowledgeDatasourceEntranceSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.entrance_id.as_uuid(), "KnowledgeDatasourceEntrance")?;
        validate_name(&spec.name)?;
        Sha256Digest::parse(spec.provenance_digest.as_str())?;
        match &spec.kind {
            KnowledgeDatasourceEntranceKindV1::FileUpload {
                user_file_id,
                content_digest,
            } => {
                validate_non_nil(user_file_id.as_uuid(), "UserFile")?;
                Sha256Digest::parse(content_digest.as_str())?;
            }
            KnowledgeDatasourceEntranceKindV1::InlineText { content } => {
                content.validate()?;
                if !content.media_type.starts_with("text/") {
                    return Err(
                        "KnowledgeDatasourceEntrance inline_text media type must be text/*".into(),
                    );
                }
            }
        }
        let kind_block = match &spec.kind {
            KnowledgeDatasourceEntranceKindV1::FileUpload {
                user_file_id,
                content_digest,
            } => BlockBuilder::new("kind")
                .attr("content_digest", string(content_digest.as_str()))
                .attr("name", string("file_upload"))
                .attr("user_file_id", string(&user_file_id.to_string()))
                .build(),
            KnowledgeDatasourceEntranceKindV1::InlineText { content } => BlockBuilder::new("kind")
                .attr("content_digest", string(content.digest.as_str()))
                .attr("media_type", string(&content.media_type))
                .attr("name", string("inline_text"))
                .attr("object_ref", string(&content.object_ref))
                .attr("size_bytes", number(content.size_bytes as f64))
                .build(),
        };
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr("entrance_id", string(&spec.entrance_id.to_string()))
                .attr(
                    "knowledge_base_id",
                    string(&spec.knowledge_base_id.to_string()),
                )
                .attr(
                    "knowledge_base_revision_id",
                    string(&spec.knowledge_base_revision_id.to_string()),
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
                .attr("schema", string(KNOWLEDGE_DATASOURCE_ENTRANCE_SCHEMA_V1))
                .nested_block(kind_block)
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
            return Err("stored KnowledgeDatasourceEntrance ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeDatasourceEntranceSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeDatasourceEntranceSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeDatasourceEntrance must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "entrance_id",
            "knowledge_base_id",
            "knowledge_base_revision_id",
            "name",
            "organization_id",
            "project_id",
            "provenance_digest",
            "schema",
        ],
        &["kind"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_DATASOURCE_ENTRANCE_SCHEMA_V1 {
        return Err("KnowledgeDatasourceEntrance schema is unsupported".into());
    }
    let kind = exact_child(root, "kind")?;
    let kind_name = required_string(kind, "name")?;
    let kind = match kind_name.as_str() {
        "file_upload" => {
            exact_shape(
                kind,
                "kind",
                &["content_digest", "name", "user_file_id"],
                &[],
            )?;
            KnowledgeDatasourceEntranceKindV1::FileUpload {
                user_file_id: UserFileId::from_uuid(required_uuid(kind, "user_file_id")?),
                content_digest: required_digest(kind, "content_digest")?,
            }
        }
        "inline_text" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "content_digest",
                    "media_type",
                    "name",
                    "object_ref",
                    "size_bytes",
                ],
                &[],
            )?;
            KnowledgeDatasourceEntranceKindV1::InlineText {
                content: KnowledgeContentReferenceV1 {
                    object_ref: required_string(kind, "object_ref")?,
                    digest: required_digest(kind, "content_digest")?,
                    size_bytes: required_u64(kind, "size_bytes", MAX_CONTENT_BYTES)?,
                    media_type: required_string(kind, "media_type")?,
                },
            }
        }
        "online_document" | "web_crawler" | "marketplace_datasource" => {
            return Err(format!(
                "KnowledgeDatasourceEntrance kind {kind_name:?} is deferred to owning gates"
            ));
        }
        _ => return Err("KnowledgeDatasourceEntrance kind is unsupported".into()),
    };
    Ok(KnowledgeDatasourceEntranceSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        entrance_id: KnowledgeDatasourceEntranceId::from_uuid(required_uuid(
            root,
            "entrance_id",
        )?),
        name: required_string(root, "name")?,
        provenance_digest: required_digest(root, "provenance_digest")?,
        kind,
    })
}

fn seal(
    spec: KnowledgeDatasourceEntranceSpecV1,
    document: Document,
) -> Result<KnowledgeDatasourceEntranceV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeDatasourceEntrance ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeDatasourceEntrance ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeDatasourceEntrance contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeDatasourceEntranceV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeDatasourceEntranceV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeDatasourceEntrance ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeDatasourceEntrance ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeDatasourceEntrance ACL is invalid: {error}"))?;
    let value = KnowledgeDatasourceEntranceV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeDatasourceEntrance ACL is not canonical".into());
    }
    Ok(value)
}
