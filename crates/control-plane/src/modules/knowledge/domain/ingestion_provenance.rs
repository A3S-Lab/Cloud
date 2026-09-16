use super::acl::{
    exact_child, exact_shape, required_digest, required_string, required_uuid, validate_name,
    validate_non_nil,
};
use super::types::{
    KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_INGESTION_PROVENANCE_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeIngestionProvenanceId, OrganizationId,
    ProjectId, Sha256Digest, UserFileId,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_ingestion_provenance";

/// Exact ingestion provenance pinned by Knowledge documents.
///
/// Digests of sealed provenance records are what `KnowledgeDocument`
/// stores in `provenance_digest`. Drive/crawl/marketplace and Tool/OCR
/// provenance kinds are refused until their owning gates supply real
/// source and processor contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeIngestionProvenanceKindV1 {
    FileUploadBuiltinText {
        user_file_id: UserFileId,
        entrance_digest: Sha256Digest,
        processor_output_contract_digest: Sha256Digest,
        admitted_content_digest: Sha256Digest,
    },
    InlineTextBuiltinText {
        entrance_digest: Sha256Digest,
        processor_output_contract_digest: Sha256Digest,
        admitted_content_digest: Sha256Digest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeIngestionProvenanceSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub provenance_id: KnowledgeIngestionProvenanceId,
    pub name: String,
    pub kind: KnowledgeIngestionProvenanceKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeIngestionProvenanceV1 {
    spec: KnowledgeIngestionProvenanceSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeIngestionProvenanceV1 {
    pub fn from_spec(spec: KnowledgeIngestionProvenanceSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.knowledge_base_id.as_uuid(), "KnowledgeBase")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.provenance_id.as_uuid(), "KnowledgeIngestionProvenance")?;
        validate_name(&spec.name)?;
        match &spec.kind {
            KnowledgeIngestionProvenanceKindV1::FileUploadBuiltinText {
                user_file_id,
                entrance_digest,
                processor_output_contract_digest,
                admitted_content_digest,
            } => {
                validate_non_nil(user_file_id.as_uuid(), "UserFile")?;
                Sha256Digest::parse(entrance_digest.as_str())?;
                Sha256Digest::parse(processor_output_contract_digest.as_str())?;
                Sha256Digest::parse(admitted_content_digest.as_str())?;
            }
            KnowledgeIngestionProvenanceKindV1::InlineTextBuiltinText {
                entrance_digest,
                processor_output_contract_digest,
                admitted_content_digest,
            } => {
                Sha256Digest::parse(entrance_digest.as_str())?;
                Sha256Digest::parse(processor_output_contract_digest.as_str())?;
                Sha256Digest::parse(admitted_content_digest.as_str())?;
            }
        }
        let kind_block = match &spec.kind {
            KnowledgeIngestionProvenanceKindV1::FileUploadBuiltinText {
                user_file_id,
                entrance_digest,
                processor_output_contract_digest,
                admitted_content_digest,
            } => BlockBuilder::new("kind")
                .attr(
                    "admitted_content_digest",
                    string(admitted_content_digest.as_str()),
                )
                .attr("entrance_digest", string(entrance_digest.as_str()))
                .attr("name", string("file_upload_builtin_text"))
                .attr(
                    "processor_output_contract_digest",
                    string(processor_output_contract_digest.as_str()),
                )
                .attr("user_file_id", string(&user_file_id.to_string()))
                .build(),
            KnowledgeIngestionProvenanceKindV1::InlineTextBuiltinText {
                entrance_digest,
                processor_output_contract_digest,
                admitted_content_digest,
            } => BlockBuilder::new("kind")
                .attr(
                    "admitted_content_digest",
                    string(admitted_content_digest.as_str()),
                )
                .attr("entrance_digest", string(entrance_digest.as_str()))
                .attr("name", string("inline_text_builtin_text"))
                .attr(
                    "processor_output_contract_digest",
                    string(processor_output_contract_digest.as_str()),
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
                .attr("provenance_id", string(&spec.provenance_id.to_string()))
                .attr("schema", string(KNOWLEDGE_INGESTION_PROVENANCE_SCHEMA_V1))
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
            return Err("stored KnowledgeIngestionProvenance ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeIngestionProvenanceSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeIngestionProvenanceSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeIngestionProvenance must contain exactly one top-level block".into());
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
            "provenance_id",
            "schema",
        ],
        &["kind"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_INGESTION_PROVENANCE_SCHEMA_V1 {
        return Err("KnowledgeIngestionProvenance schema is unsupported".into());
    }
    let kind = exact_child(root, "kind")?;
    let kind_name = required_string(kind, "name")?;
    let kind = match kind_name.as_str() {
        "file_upload_builtin_text" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "admitted_content_digest",
                    "entrance_digest",
                    "name",
                    "processor_output_contract_digest",
                    "user_file_id",
                ],
                &[],
            )?;
            KnowledgeIngestionProvenanceKindV1::FileUploadBuiltinText {
                user_file_id: UserFileId::from_uuid(required_uuid(kind, "user_file_id")?),
                entrance_digest: required_digest(kind, "entrance_digest")?,
                processor_output_contract_digest: required_digest(
                    kind,
                    "processor_output_contract_digest",
                )?,
                admitted_content_digest: required_digest(kind, "admitted_content_digest")?,
            }
        }
        "inline_text_builtin_text" => {
            exact_shape(
                kind,
                "kind",
                &[
                    "admitted_content_digest",
                    "entrance_digest",
                    "name",
                    "processor_output_contract_digest",
                ],
                &[],
            )?;
            KnowledgeIngestionProvenanceKindV1::InlineTextBuiltinText {
                entrance_digest: required_digest(kind, "entrance_digest")?,
                processor_output_contract_digest: required_digest(
                    kind,
                    "processor_output_contract_digest",
                )?,
                admitted_content_digest: required_digest(kind, "admitted_content_digest")?,
            }
        }
        "online_document"
        | "web_crawler"
        | "marketplace_datasource"
        | "tool_processor"
        | "ocr_layout" => {
            return Err(format!(
                "KnowledgeIngestionProvenance kind {kind_name:?} is deferred to owning gates"
            ));
        }
        _ => return Err("KnowledgeIngestionProvenance kind is unsupported".into()),
    };
    Ok(KnowledgeIngestionProvenanceSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_id: KnowledgeBaseId::from_uuid(required_uuid(root, "knowledge_base_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        provenance_id: KnowledgeIngestionProvenanceId::from_uuid(required_uuid(
            root,
            "provenance_id",
        )?),
        name: required_string(root, "name")?,
        kind,
    })
}

fn seal(
    spec: KnowledgeIngestionProvenanceSpecV1,
    document: Document,
) -> Result<KnowledgeIngestionProvenanceV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeIngestionProvenance ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeIngestionProvenance ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeIngestionProvenance contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeIngestionProvenanceV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeIngestionProvenanceV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeIngestionProvenance ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeIngestionProvenance ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeIngestionProvenance ACL is invalid: {error}"))?;
    let value = KnowledgeIngestionProvenanceV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeIngestionProvenance ACL is not canonical".into());
    }
    Ok(value)
}
