use super::acl::{
    exact_child, exact_shape, required_digest, required_string, required_uuid, validate_name,
    validate_non_nil,
};
use super::types::{
    KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_FAILURE_CLEANUP_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeDocumentId,
    KnowledgeDocumentIncrementalUpdateId, KnowledgeFailureCleanupId, OrganizationId, ProjectId,
    Sha256Digest, UserFileId,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_failure_cleanup";

/// Exact cleanup intent after failed Knowledge file/text ingestion work.
///
/// File-upload, immutable-object/inline, and document incremental-update
/// failure cleanup intents are sealed here. Drive/crawl/marketplace kinds
/// remain deferred. No cleanup worker, saga, persistence, or product-gate
/// flip is claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeFailureCleanupKindV1 {
    FailedAdmittedUserFileIngestion {
        document_id: KnowledgeDocumentId,
        provenance_digest: Sha256Digest,
        content_digest: Sha256Digest,
        user_file_id: UserFileId,
        failure_digest: Sha256Digest,
    },
    FailedImmutableObjectIngestion {
        document_id: KnowledgeDocumentId,
        provenance_digest: Sha256Digest,
        content_digest: Sha256Digest,
        failure_digest: Sha256Digest,
    },
    FailedDocumentIncrementalUpdate {
        document_id: KnowledgeDocumentId,
        update_id: KnowledgeDocumentIncrementalUpdateId,
        previous_provenance_digest: Sha256Digest,
        next_provenance_digest: Sha256Digest,
        failure_digest: Sha256Digest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeFailureCleanupSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub cleanup_id: KnowledgeFailureCleanupId,
    pub name: String,
    pub kind: KnowledgeFailureCleanupKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeFailureCleanupV1 {
    spec: KnowledgeFailureCleanupSpecV1,
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
        return Err(format!("KnowledgeFailureCleanup {label} must change"));
    }
    Ok(())
}

impl KnowledgeFailureCleanupV1 {
    pub fn from_spec(spec: KnowledgeFailureCleanupSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.cleanup_id.as_uuid(), "KnowledgeFailureCleanup")?;
        validate_name(&spec.name)?;
        match &spec.kind {
            KnowledgeFailureCleanupKindV1::FailedAdmittedUserFileIngestion {
                document_id,
                provenance_digest,
                content_digest,
                user_file_id,
                failure_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                validate_non_nil(user_file_id.as_uuid(), "UserFile")?;
                Sha256Digest::parse(provenance_digest.as_str())?;
                Sha256Digest::parse(content_digest.as_str())?;
                Sha256Digest::parse(failure_digest.as_str())?;
            }
            KnowledgeFailureCleanupKindV1::FailedImmutableObjectIngestion {
                document_id,
                provenance_digest,
                content_digest,
                failure_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                Sha256Digest::parse(provenance_digest.as_str())?;
                Sha256Digest::parse(content_digest.as_str())?;
                Sha256Digest::parse(failure_digest.as_str())?;
            }
            KnowledgeFailureCleanupKindV1::FailedDocumentIncrementalUpdate {
                document_id,
                update_id,
                previous_provenance_digest,
                next_provenance_digest,
                failure_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                validate_non_nil(update_id.as_uuid(), "KnowledgeDocumentIncrementalUpdate")?;
                validate_digest_pair(
                    previous_provenance_digest,
                    next_provenance_digest,
                    "provenance_digest",
                )?;
                Sha256Digest::parse(failure_digest.as_str())?;
            }
        }
        let kind_block = match &spec.kind {
            KnowledgeFailureCleanupKindV1::FailedAdmittedUserFileIngestion {
                document_id,
                provenance_digest,
                content_digest,
                user_file_id,
                failure_digest,
            } => BlockBuilder::new("kind")
                .attr("content_digest", string(content_digest.as_str()))
                .attr("document_id", string(&document_id.to_string()))
                .attr("failure_digest", string(failure_digest.as_str()))
                .attr("name", string("failed_admitted_user_file_ingestion"))
                .attr("provenance_digest", string(provenance_digest.as_str()))
                .attr("user_file_id", string(&user_file_id.to_string()))
                .build(),
            KnowledgeFailureCleanupKindV1::FailedImmutableObjectIngestion {
                document_id,
                provenance_digest,
                content_digest,
                failure_digest,
            } => BlockBuilder::new("kind")
                .attr("content_digest", string(content_digest.as_str()))
                .attr("document_id", string(&document_id.to_string()))
                .attr("failure_digest", string(failure_digest.as_str()))
                .attr("name", string("failed_immutable_object_ingestion"))
                .attr("provenance_digest", string(provenance_digest.as_str()))
                .build(),
            KnowledgeFailureCleanupKindV1::FailedDocumentIncrementalUpdate {
                document_id,
                update_id,
                previous_provenance_digest,
                next_provenance_digest,
                failure_digest,
            } => BlockBuilder::new("kind")
                .attr("document_id", string(&document_id.to_string()))
                .attr("failure_digest", string(failure_digest.as_str()))
                .attr("name", string("failed_document_incremental_update"))
                .attr(
                    "next_provenance_digest",
                    string(next_provenance_digest.as_str()),
                )
                .attr(
                    "previous_provenance_digest",
                    string(previous_provenance_digest.as_str()),
                )
                .attr("update_id", string(&update_id.to_string()))
                .build(),
        };
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr("cleanup_id", string(&spec.cleanup_id.to_string()))
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
                .attr("schema", string(KNOWLEDGE_FAILURE_CLEANUP_SCHEMA_V1))
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
            return Err("KnowledgeFailureCleanup digest does not match".into());
        }
        Ok(value)
    }

    pub fn spec(&self) -> &KnowledgeFailureCleanupSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeFailureCleanupSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeFailureCleanup must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "cleanup_id",
            "knowledge_base_id",
            "knowledge_base_revision_id",
            "name",
            "organization_id",
            "project_id",
            "schema",
        ],
        &["kind"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_FAILURE_CLEANUP_SCHEMA_V1 {
        return Err("KnowledgeFailureCleanup schema is unsupported".into());
    }
    let kind = exact_child(root, "kind")?;
    let kind_name = required_string(kind, "name")?;
    let parsed_kind = match kind_name.as_str() {
        "failed_admitted_user_file_ingestion" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "content_digest",
                    "document_id",
                    "failure_digest",
                    "name",
                    "provenance_digest",
                    "user_file_id",
                ],
                &[],
            )?;
            KnowledgeFailureCleanupKindV1::FailedAdmittedUserFileIngestion {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                provenance_digest: required_digest(kind, "provenance_digest")?,
                content_digest: required_digest(kind, "content_digest")?,
                user_file_id: UserFileId::from_uuid(required_uuid(kind, "user_file_id")?),
                failure_digest: required_digest(kind, "failure_digest")?,
            }
        }
        "failed_immutable_object_ingestion" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "content_digest",
                    "document_id",
                    "failure_digest",
                    "name",
                    "provenance_digest",
                ],
                &[],
            )?;
            KnowledgeFailureCleanupKindV1::FailedImmutableObjectIngestion {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                provenance_digest: required_digest(kind, "provenance_digest")?,
                content_digest: required_digest(kind, "content_digest")?,
                failure_digest: required_digest(kind, "failure_digest")?,
            }
        }
        "failed_document_incremental_update" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "document_id",
                    "failure_digest",
                    "name",
                    "next_provenance_digest",
                    "previous_provenance_digest",
                    "update_id",
                ],
                &[],
            )?;
            KnowledgeFailureCleanupKindV1::FailedDocumentIncrementalUpdate {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                update_id: KnowledgeDocumentIncrementalUpdateId::from_uuid(required_uuid(
                    kind,
                    "update_id",
                )?),
                previous_provenance_digest: required_digest(kind, "previous_provenance_digest")?,
                next_provenance_digest: required_digest(kind, "next_provenance_digest")?,
                failure_digest: required_digest(kind, "failure_digest")?,
            }
        }
        "online_document" | "web_crawler" | "marketplace_datasource" => {
            return Err(format!(
                "KnowledgeFailureCleanup kind {kind_name:?} is deferred to owning gates"
            ));
        }
        _ => return Err("KnowledgeFailureCleanup kind is unsupported".into()),
    };
    Ok(KnowledgeFailureCleanupSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        cleanup_id: KnowledgeFailureCleanupId::from_uuid(required_uuid(root, "cleanup_id")?),
        name: required_string(root, "name")?,
        kind: parsed_kind,
    })
}

fn seal(
    spec: KnowledgeFailureCleanupSpecV1,
    document: Document,
) -> Result<KnowledgeFailureCleanupV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeFailureCleanup ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeFailureCleanup ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeFailureCleanup contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeFailureCleanupV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeFailureCleanupV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeFailureCleanup ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeFailureCleanup ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeFailureCleanup ACL is invalid: {error}"))?;
    let value = KnowledgeFailureCleanupV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeFailureCleanup ACL is not canonical".into());
    }
    Ok(value)
}
