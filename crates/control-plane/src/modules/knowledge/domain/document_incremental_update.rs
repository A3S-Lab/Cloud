use super::acl::{
    exact_child, exact_shape, required_digest, required_string, required_uuid, validate_name,
    validate_non_nil,
};
use super::types::{
    KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_DOCUMENT_INCREMENTAL_UPDATE_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeDocumentId,
    KnowledgeDocumentIncrementalUpdateId, OrganizationId, ProjectId, Sha256Digest, UserFileId,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_document_incremental_update";

/// Exact incremental source-content update for an existing Knowledge document.
///
/// File-upload and immutable-object/inline replacements composed with
/// Knowledge-owned provenance are sealed here. Drive/crawl/marketplace
/// update kinds remain deferred. No worker, saga, persistence, or
/// product-gate flip is claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeDocumentIncrementalUpdateKindV1 {
    ReplaceAdmittedUserFile {
        document_id: KnowledgeDocumentId,
        previous_provenance_digest: Sha256Digest,
        next_provenance_digest: Sha256Digest,
        previous_content_digest: Sha256Digest,
        next_content_digest: Sha256Digest,
        user_file_id: UserFileId,
    },
    ReplaceImmutableObject {
        document_id: KnowledgeDocumentId,
        previous_provenance_digest: Sha256Digest,
        next_provenance_digest: Sha256Digest,
        previous_content_digest: Sha256Digest,
        next_content_digest: Sha256Digest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDocumentIncrementalUpdateSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub update_id: KnowledgeDocumentIncrementalUpdateId,
    pub name: String,
    pub kind: KnowledgeDocumentIncrementalUpdateKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDocumentIncrementalUpdateV1 {
    spec: KnowledgeDocumentIncrementalUpdateSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

fn validate_digest_pair(
    previous: &Sha256Digest,
    next: &Sha256Digest,
    label: &str,
) -> Result<(), String> {
    Sha256Digest::parse(previous.as_str())?;
    Sha256Digest::parse(next.as_str())?;
    if previous.as_str() == next.as_str() {
        return Err(format!(
            "KnowledgeDocumentIncrementalUpdate {label} must change"
        ));
    }
    Ok(())
}

impl KnowledgeDocumentIncrementalUpdateV1 {
    pub fn from_spec(spec: KnowledgeDocumentIncrementalUpdateSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.update_id.as_uuid(), "KnowledgeDocumentIncrementalUpdate")?;
        validate_name(&spec.name)?;
        match &spec.kind {
            KnowledgeDocumentIncrementalUpdateKindV1::ReplaceAdmittedUserFile {
                document_id,
                previous_provenance_digest,
                next_provenance_digest,
                previous_content_digest,
                next_content_digest,
                user_file_id,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                validate_non_nil(user_file_id.as_uuid(), "UserFile")?;
                validate_digest_pair(
                    previous_provenance_digest,
                    next_provenance_digest,
                    "provenance_digest",
                )?;
                validate_digest_pair(
                    previous_content_digest,
                    next_content_digest,
                    "content_digest",
                )?;
            }
            KnowledgeDocumentIncrementalUpdateKindV1::ReplaceImmutableObject {
                document_id,
                previous_provenance_digest,
                next_provenance_digest,
                previous_content_digest,
                next_content_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                validate_digest_pair(
                    previous_provenance_digest,
                    next_provenance_digest,
                    "provenance_digest",
                )?;
                validate_digest_pair(
                    previous_content_digest,
                    next_content_digest,
                    "content_digest",
                )?;
            }
        }
        let kind_block = match &spec.kind {
            KnowledgeDocumentIncrementalUpdateKindV1::ReplaceAdmittedUserFile {
                document_id,
                previous_provenance_digest,
                next_provenance_digest,
                previous_content_digest,
                next_content_digest,
                user_file_id,
            } => BlockBuilder::new("kind")
                .attr("document_id", string(&document_id.to_string()))
                .attr("name", string("replace_admitted_user_file"))
                .attr(
                    "next_content_digest",
                    string(next_content_digest.as_str()),
                )
                .attr(
                    "next_provenance_digest",
                    string(next_provenance_digest.as_str()),
                )
                .attr(
                    "previous_content_digest",
                    string(previous_content_digest.as_str()),
                )
                .attr(
                    "previous_provenance_digest",
                    string(previous_provenance_digest.as_str()),
                )
                .attr("user_file_id", string(&user_file_id.to_string()))
                .build(),
            KnowledgeDocumentIncrementalUpdateKindV1::ReplaceImmutableObject {
                document_id,
                previous_provenance_digest,
                next_provenance_digest,
                previous_content_digest,
                next_content_digest,
            } => BlockBuilder::new("kind")
                .attr("document_id", string(&document_id.to_string()))
                .attr("name", string("replace_immutable_object"))
                .attr(
                    "next_content_digest",
                    string(next_content_digest.as_str()),
                )
                .attr(
                    "next_provenance_digest",
                    string(next_provenance_digest.as_str()),
                )
                .attr(
                    "previous_content_digest",
                    string(previous_content_digest.as_str()),
                )
                .attr(
                    "previous_provenance_digest",
                    string(previous_provenance_digest.as_str()),
                )
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
                .attr(
                    "schema",
                    string(KNOWLEDGE_DOCUMENT_INCREMENTAL_UPDATE_SCHEMA_V1),
                )
                .attr("update_id", string(&spec.update_id.to_string()))
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
            return Err("KnowledgeDocumentIncrementalUpdate digest does not match".into());
        }
        Ok(value)
    }

    pub fn spec(&self) -> &KnowledgeDocumentIncrementalUpdateSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeDocumentIncrementalUpdateSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err(
            "KnowledgeDocumentIncrementalUpdate must contain exactly one top-level block".into(),
        );
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
            "update_id",
        ],
        &["kind"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_DOCUMENT_INCREMENTAL_UPDATE_SCHEMA_V1 {
        return Err("KnowledgeDocumentIncrementalUpdate schema is unsupported".into());
    }
    let kind = exact_child(root, "kind")?;
    let kind_name = required_string(kind, "name")?;
    let parsed_kind = match kind_name.as_str() {
        "replace_admitted_user_file" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "document_id",
                    "name",
                    "next_content_digest",
                    "next_provenance_digest",
                    "previous_content_digest",
                    "previous_provenance_digest",
                    "user_file_id",
                ],
                &[],
            )?;
            KnowledgeDocumentIncrementalUpdateKindV1::ReplaceAdmittedUserFile {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                previous_provenance_digest: required_digest(kind, "previous_provenance_digest")?,
                next_provenance_digest: required_digest(kind, "next_provenance_digest")?,
                previous_content_digest: required_digest(kind, "previous_content_digest")?,
                next_content_digest: required_digest(kind, "next_content_digest")?,
                user_file_id: UserFileId::from_uuid(required_uuid(kind, "user_file_id")?),
            }
        }
        "replace_immutable_object" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "document_id",
                    "name",
                    "next_content_digest",
                    "next_provenance_digest",
                    "previous_content_digest",
                    "previous_provenance_digest",
                ],
                &[],
            )?;
            KnowledgeDocumentIncrementalUpdateKindV1::ReplaceImmutableObject {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                previous_provenance_digest: required_digest(kind, "previous_provenance_digest")?,
                next_provenance_digest: required_digest(kind, "next_provenance_digest")?,
                previous_content_digest: required_digest(kind, "previous_content_digest")?,
                next_content_digest: required_digest(kind, "next_content_digest")?,
            }
        }
        "online_document" | "web_crawler" | "marketplace_datasource" => {
            return Err(format!(
                "KnowledgeDocumentIncrementalUpdate kind {kind_name:?} is deferred to owning gates"
            ));
        }
        _ => return Err("KnowledgeDocumentIncrementalUpdate kind is unsupported".into()),
    };
    Ok(KnowledgeDocumentIncrementalUpdateSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        update_id: KnowledgeDocumentIncrementalUpdateId::from_uuid(required_uuid(
            root,
            "update_id",
        )?),
        name: required_string(root, "name")?,
        kind: parsed_kind,
    })
}

fn seal(
    spec: KnowledgeDocumentIncrementalUpdateSpecV1,
    document: Document,
) -> Result<KnowledgeDocumentIncrementalUpdateV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeDocumentIncrementalUpdate ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeDocumentIncrementalUpdate ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeDocumentIncrementalUpdate contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeDocumentIncrementalUpdateV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeDocumentIncrementalUpdateV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeDocumentIncrementalUpdate ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err(
            "KnowledgeDocumentIncrementalUpdate ACL contains a bare carriage return".into(),
        );
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized).map_err(|error| {
        format!("KnowledgeDocumentIncrementalUpdate ACL is invalid: {error}")
    })?;
    let value = KnowledgeDocumentIncrementalUpdateV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeDocumentIncrementalUpdate ACL is not canonical".into());
    }
    Ok(value)
}
