use super::acl::{
    exact_child, exact_shape, required_digest, required_string, required_uuid, validate_name,
    validate_non_nil,
};
use super::types::{
    KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_INGESTION_CANCELLATION_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeDocumentId,
    KnowledgeDocumentIncrementalUpdateId, KnowledgeIngestionCancellationId, OrganizationId,
    ProjectId, Sha256Digest, UserFileId,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_ingestion_cancellation";

/// Exact cancellation intent for in-flight Knowledge file/text ingestion work.
///
/// File-upload, immutable-object/inline, and document incremental-update
/// cancellation intents are sealed here. Drive/crawl/marketplace kinds remain
/// deferred. No cancellation worker, saga, persistence, or product-gate flip
/// is claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeIngestionCancellationKindV1 {
    CancelAdmittedUserFileIngestion {
        document_id: KnowledgeDocumentId,
        provenance_digest: Sha256Digest,
        content_digest: Sha256Digest,
        user_file_id: UserFileId,
    },
    CancelImmutableObjectIngestion {
        document_id: KnowledgeDocumentId,
        provenance_digest: Sha256Digest,
        content_digest: Sha256Digest,
    },
    CancelDocumentIncrementalUpdate {
        document_id: KnowledgeDocumentId,
        update_id: KnowledgeDocumentIncrementalUpdateId,
        previous_provenance_digest: Sha256Digest,
        next_provenance_digest: Sha256Digest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeIngestionCancellationSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub cancellation_id: KnowledgeIngestionCancellationId,
    pub name: String,
    pub kind: KnowledgeIngestionCancellationKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeIngestionCancellationV1 {
    spec: KnowledgeIngestionCancellationSpecV1,
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
            "KnowledgeIngestionCancellation {label} must change"
        ));
    }
    Ok(())
}

impl KnowledgeIngestionCancellationV1 {
    pub fn from_spec(spec: KnowledgeIngestionCancellationSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.cancellation_id.as_uuid(), "KnowledgeIngestionCancellation")?;
        validate_name(&spec.name)?;
        match &spec.kind {
            KnowledgeIngestionCancellationKindV1::CancelAdmittedUserFileIngestion {
                document_id,
                provenance_digest,
                content_digest,
                user_file_id,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                validate_non_nil(user_file_id.as_uuid(), "UserFile")?;
                Sha256Digest::parse(provenance_digest.as_str())?;
                Sha256Digest::parse(content_digest.as_str())?;
            }
            KnowledgeIngestionCancellationKindV1::CancelImmutableObjectIngestion {
                document_id,
                provenance_digest,
                content_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                Sha256Digest::parse(provenance_digest.as_str())?;
                Sha256Digest::parse(content_digest.as_str())?;
            }
            KnowledgeIngestionCancellationKindV1::CancelDocumentIncrementalUpdate {
                document_id,
                update_id,
                previous_provenance_digest,
                next_provenance_digest,
            } => {
                validate_non_nil(document_id.as_uuid(), "KnowledgeDocument")?;
                validate_non_nil(update_id.as_uuid(), "KnowledgeDocumentIncrementalUpdate")?;
                validate_digest_pair(
                    previous_provenance_digest,
                    next_provenance_digest,
                    "provenance_digest",
                )?;
            }
        }
        let kind_block = match &spec.kind {
            KnowledgeIngestionCancellationKindV1::CancelAdmittedUserFileIngestion {
                document_id,
                provenance_digest,
                content_digest,
                user_file_id,
            } => BlockBuilder::new("kind")
                .attr("content_digest", string(content_digest.as_str()))
                .attr("document_id", string(&document_id.to_string()))
                .attr("name", string("cancel_admitted_user_file_ingestion"))
                .attr("provenance_digest", string(provenance_digest.as_str()))
                .attr("user_file_id", string(&user_file_id.to_string()))
                .build(),
            KnowledgeIngestionCancellationKindV1::CancelImmutableObjectIngestion {
                document_id,
                provenance_digest,
                content_digest,
            } => BlockBuilder::new("kind")
                .attr("content_digest", string(content_digest.as_str()))
                .attr("document_id", string(&document_id.to_string()))
                .attr("name", string("cancel_immutable_object_ingestion"))
                .attr("provenance_digest", string(provenance_digest.as_str()))
                .build(),
            KnowledgeIngestionCancellationKindV1::CancelDocumentIncrementalUpdate {
                document_id,
                update_id,
                previous_provenance_digest,
                next_provenance_digest,
            } => BlockBuilder::new("kind")
                .attr("document_id", string(&document_id.to_string()))
                .attr("name", string("cancel_document_incremental_update"))
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
                .attr(
                    "cancellation_id",
                    string(&spec.cancellation_id.to_string()),
                )
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
                    string(KNOWLEDGE_INGESTION_CANCELLATION_SCHEMA_V1),
                )
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
            return Err("KnowledgeIngestionCancellation digest does not match".into());
        }
        Ok(value)
    }

    pub fn spec(&self) -> &KnowledgeIngestionCancellationSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeIngestionCancellationSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err(
            "KnowledgeIngestionCancellation must contain exactly one top-level block".into(),
        );
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "cancellation_id",
            "knowledge_base_id",
            "knowledge_base_revision_id",
            "name",
            "organization_id",
            "project_id",
            "schema",
        ],
        &["kind"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_INGESTION_CANCELLATION_SCHEMA_V1 {
        return Err("KnowledgeIngestionCancellation schema is unsupported".into());
    }
    let kind = exact_child(root, "kind")?;
    let kind_name = required_string(kind, "name")?;
    let parsed_kind = match kind_name.as_str() {
        "cancel_admitted_user_file_ingestion" => {
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
            KnowledgeIngestionCancellationKindV1::CancelAdmittedUserFileIngestion {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                provenance_digest: required_digest(kind, "provenance_digest")?,
                content_digest: required_digest(kind, "content_digest")?,
                user_file_id: UserFileId::from_uuid(required_uuid(kind, "user_file_id")?),
            }
        }
        "cancel_immutable_object_ingestion" => {
            exact_shape(
                kind,
                "kind",
                &["content_digest", "document_id", "name", "provenance_digest"],
                &[],
            )?;
            KnowledgeIngestionCancellationKindV1::CancelImmutableObjectIngestion {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                provenance_digest: required_digest(kind, "provenance_digest")?,
                content_digest: required_digest(kind, "content_digest")?,
            }
        }
        "cancel_document_incremental_update" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "document_id",
                    "name",
                    "next_provenance_digest",
                    "previous_provenance_digest",
                    "update_id",
                ],
                &[],
            )?;
            KnowledgeIngestionCancellationKindV1::CancelDocumentIncrementalUpdate {
                document_id: KnowledgeDocumentId::from_uuid(required_uuid(kind, "document_id")?),
                update_id: KnowledgeDocumentIncrementalUpdateId::from_uuid(required_uuid(
                    kind,
                    "update_id",
                )?),
                previous_provenance_digest: required_digest(kind, "previous_provenance_digest")?,
                next_provenance_digest: required_digest(kind, "next_provenance_digest")?,
            }
        }
        "online_document" | "web_crawler" | "marketplace_datasource" => {
            return Err(format!(
                "KnowledgeIngestionCancellation kind {kind_name:?} is deferred to owning gates"
            ));
        }
        _ => return Err("KnowledgeIngestionCancellation kind is unsupported".into()),
    };
    Ok(KnowledgeIngestionCancellationSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        cancellation_id: KnowledgeIngestionCancellationId::from_uuid(required_uuid(
            root,
            "cancellation_id",
        )?),
        name: required_string(root, "name")?,
        kind: parsed_kind,
    })
}

fn seal(
    spec: KnowledgeIngestionCancellationSpecV1,
    document: Document,
) -> Result<KnowledgeIngestionCancellationV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeIngestionCancellation ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeIngestionCancellation ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeIngestionCancellation contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeIngestionCancellationV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeIngestionCancellationV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeIngestionCancellation ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err(
            "KnowledgeIngestionCancellation ACL contains a bare carriage return".into(),
        );
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized).map_err(|error| {
        format!("KnowledgeIngestionCancellation ACL is invalid: {error}")
    })?;
    let value = KnowledgeIngestionCancellationV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeIngestionCancellation ACL is not canonical".into());
    }
    Ok(value)
}
