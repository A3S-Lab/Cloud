use super::acl::{
    exact_child, exact_shape, required_digest, required_string, required_uuid, validate_name,
    validate_non_nil,
};
use super::types::{
    KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_SOURCE_TOMBSTONE_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeDocumentId, KnowledgeSourceTombstoneId,
    OrganizationId, ProjectId, Sha256Digest, UserFileId,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_source_tombstone";

/// Exact source tombstone pinned when a Knowledge document is deleted.
///
/// File-upload and immutable-object/inline sources composed with Knowledge-
/// owned provenance are sealed here. Drive/crawl/marketplace source
/// tombstones remain deferred to their owning gates. No cleanup worker,
/// saga, or product-gate flip is claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeSourceTombstoneKindV1 {
    AdmittedUserFile {
        document_id: KnowledgeDocumentId,
        provenance_digest: Sha256Digest,
        user_file_id: UserFileId,
        content_digest: Sha256Digest,
    },
    ImmutableObject {
        document_id: KnowledgeDocumentId,
        provenance_digest: Sha256Digest,
        content_digest: Sha256Digest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSourceTombstoneSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub tombstone_id: KnowledgeSourceTombstoneId,
    pub name: String,
    pub kind: KnowledgeSourceTombstoneKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSourceTombstoneV1 {
    spec: KnowledgeSourceTombstoneSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeSourceTombstoneV1 {
    pub fn from_spec(spec: KnowledgeSourceTombstoneSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.tombstone_id.as_uuid(), "KnowledgeSourceTombstone")?;
        validate_name(&spec.name)?;
        match &spec.kind {
            KnowledgeSourceTombstoneKindV1::AdmittedUserFile {
                document_id,
                provenance_digest,
                user_file_id,
                content_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                validate_non_nil(user_file_id.as_uuid(), "UserFile")?;
                Sha256Digest::parse(provenance_digest.as_str())?;
                Sha256Digest::parse(content_digest.as_str())?;
            }
            KnowledgeSourceTombstoneKindV1::ImmutableObject {
                document_id,
                provenance_digest,
                content_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                Sha256Digest::parse(provenance_digest.as_str())?;
                Sha256Digest::parse(content_digest.as_str())?;
            }
        }
        let kind_block = match &spec.kind {
            KnowledgeSourceTombstoneKindV1::AdmittedUserFile {
                document_id,
                provenance_digest,
                user_file_id,
                content_digest,
            } => BlockBuilder::new("kind")
                .attr("content_digest", string(content_digest.as_str()))
                .attr("document_id", string(&document_id.to_string()))
                .attr("name", string("admitted_user_file"))
                .attr("provenance_digest", string(provenance_digest.as_str()))
                .attr("user_file_id", string(&user_file_id.to_string()))
                .build(),
            KnowledgeSourceTombstoneKindV1::ImmutableObject {
                document_id,
                provenance_digest,
                content_digest,
            } => BlockBuilder::new("kind")
                .attr("content_digest", string(content_digest.as_str()))
                .attr("document_id", string(&document_id.to_string()))
                .attr("name", string("immutable_object"))
                .attr("provenance_digest", string(provenance_digest.as_str()))
                .build(),
        };
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
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
                .attr("schema", string(KNOWLEDGE_SOURCE_TOMBSTONE_SCHEMA_V1))
                .attr("tombstone_id", string(&spec.tombstone_id.to_string()))
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
            return Err("stored KnowledgeSourceTombstone ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeSourceTombstoneSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeSourceTombstoneSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeSourceTombstone must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "knowledge_base_id",
            "knowledge_base_revision_id",
            "name",
            "organization_id",
            "project_id",
            "schema",
            "tombstone_id",
        ],
        &["kind"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_SOURCE_TOMBSTONE_SCHEMA_V1 {
        return Err("KnowledgeSourceTombstone schema is unsupported".into());
    }
    let kind = exact_child(root, "kind")?;
    let kind_name = required_string(kind, "name")?;
    let parsed_kind = match kind_name.as_str() {
        "admitted_user_file" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "content_digest",
                    "document_id",
                    "name",
                    "provenance_digest",
                    "user_file_id",
                ],
                &[],
            )?;
            KnowledgeSourceTombstoneKindV1::AdmittedUserFile {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                provenance_digest: required_digest(kind, "provenance_digest")?,
                user_file_id: UserFileId::from_uuid(required_uuid(kind, "user_file_id")?),
                content_digest: required_digest(kind, "content_digest")?,
            }
        }
        "immutable_object" => {
            exact_shape(
                kind,
                "kind",
                &["content_digest", "document_id", "name", "provenance_digest"],
                &[],
            )?;
            KnowledgeSourceTombstoneKindV1::ImmutableObject {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                provenance_digest: required_digest(kind, "provenance_digest")?,
                content_digest: required_digest(kind, "content_digest")?,
            }
        }
        "online_document" | "web_crawler" | "marketplace_datasource" => {
            return Err(format!(
                "KnowledgeSourceTombstone kind {kind_name:?} is deferred to owning gates"
            ));
        }
        _ => return Err("KnowledgeSourceTombstone kind is unsupported".into()),
    };
    Ok(KnowledgeSourceTombstoneSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        tombstone_id: KnowledgeSourceTombstoneId::from_uuid(required_uuid(root, "tombstone_id")?),
        name: required_string(root, "name")?,
        kind: parsed_kind,
    })
}

fn seal(
    spec: KnowledgeSourceTombstoneSpecV1,
    document: Document,
) -> Result<KnowledgeSourceTombstoneV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeSourceTombstone ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeSourceTombstone ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeSourceTombstone contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeSourceTombstoneV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeSourceTombstoneV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeSourceTombstone ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeSourceTombstone ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeSourceTombstone ACL is invalid: {error}"))?;
    let value = KnowledgeSourceTombstoneV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeSourceTombstone ACL is not canonical".into());
    }
    Ok(value)
}
