use super::acl::{
    exact_child, exact_shape, required_string, required_u64, required_uuid, validate_name,
    validate_non_nil,
};
use super::types::{
    KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_PROCESSOR_OUTPUT_CONTRACT_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeProcessorOutputContractId, OrganizationId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_processor_output_contract";
const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;

/// Built-in plain-text processor output contract pinned by pipeline releases.
///
/// Digests of sealed contracts are what `KnowledgePipelineRelease` stores in
/// `output_contract_digest`. Tool, OCR/layout, and multimodal processors are
/// refused here until their owning gates supply real execution contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnowledgeProcessorOutputContractKindV1 {
    BuiltinPlainTextExtract {
        max_input_bytes: u64,
        output_media_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProcessorOutputContractSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub contract_id: KnowledgeProcessorOutputContractId,
    pub name: String,
    pub kind: KnowledgeProcessorOutputContractKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProcessorOutputContractV1 {
    spec: KnowledgeProcessorOutputContractSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeProcessorOutputContractV1 {
    pub fn from_spec(spec: KnowledgeProcessorOutputContractSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.contract_id.as_uuid(), "KnowledgeProcessorOutputContract")?;
        validate_name(&spec.name)?;
        match &spec.kind {
            KnowledgeProcessorOutputContractKindV1::BuiltinPlainTextExtract {
                max_input_bytes,
                output_media_type,
            } => {
                if *max_input_bytes == 0 || *max_input_bytes > MAX_INPUT_BYTES {
                    return Err(
                        "KnowledgeProcessorOutputContract max_input_bytes is out of bounds".into(),
                    );
                }
                if !output_media_type.starts_with("text/") {
                    return Err(
                        "KnowledgeProcessorOutputContract builtin plain-text extract output media type must be text/*"
                            .into(),
                    );
                }
            }
        }
        let kind_block = match &spec.kind {
            KnowledgeProcessorOutputContractKindV1::BuiltinPlainTextExtract {
                max_input_bytes,
                output_media_type,
            } => BlockBuilder::new("kind")
                .attr("max_input_bytes", number(*max_input_bytes as f64))
                .attr("name", string("builtin_plain_text_extract"))
                .attr("output_media_type", string(output_media_type))
                .build(),
        };
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr("contract_id", string(&spec.contract_id.to_string()))
                .attr("name", string(&spec.name))
                .attr(
                    "organization_id",
                    string(&spec.organization_id.to_string()),
                )
                .attr("project_id", string(&spec.project_id.to_string()))
                .attr(
                    "schema",
                    string(KNOWLEDGE_PROCESSOR_OUTPUT_CONTRACT_SCHEMA_V1),
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
            return Err(
                "stored KnowledgeProcessorOutputContract ACL and digest do not match".into(),
            );
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeProcessorOutputContractSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgeProcessorOutputContractSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err(
            "KnowledgeProcessorOutputContract must contain exactly one top-level block".into(),
        );
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "contract_id",
            "name",
            "organization_id",
            "project_id",
            "schema",
        ],
        &["kind"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_PROCESSOR_OUTPUT_CONTRACT_SCHEMA_V1 {
        return Err("KnowledgeProcessorOutputContract schema is unsupported".into());
    }
    let kind = exact_child(root, "kind")?;
    let kind_name = required_string(kind, "name")?;
    let kind = match kind_name.as_str() {
        "builtin_plain_text_extract" => {
            exact_shape(
                kind,
                "kind",
                &["max_input_bytes", "name", "output_media_type"],
                &[],
            )?;
            KnowledgeProcessorOutputContractKindV1::BuiltinPlainTextExtract {
                max_input_bytes: required_u64(kind, "max_input_bytes", MAX_INPUT_BYTES)?,
                output_media_type: required_string(kind, "output_media_type")?,
            }
        }
        "tool_processor" | "ocr_layout" | "multimodal_attachment" => {
            return Err(format!(
                "KnowledgeProcessorOutputContract kind {kind_name:?} is deferred to owning gates"
            ));
        }
        _ => return Err("KnowledgeProcessorOutputContract kind is unsupported".into()),
    };
    Ok(KnowledgeProcessorOutputContractSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        contract_id: KnowledgeProcessorOutputContractId::from_uuid(required_uuid(
            root,
            "contract_id",
        )?),
        name: required_string(root, "name")?,
        kind,
    })
}

fn seal(
    spec: KnowledgeProcessorOutputContractSpecV1,
    document: Document,
) -> Result<KnowledgeProcessorOutputContractV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeProcessorOutputContract ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl).map_err(|error| {
        format!("generated KnowledgeProcessorOutputContract ACL is invalid: {error}")
    })?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeProcessorOutputContract contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeProcessorOutputContractV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeProcessorOutputContractV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeProcessorOutputContract ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err(
            "KnowledgeProcessorOutputContract ACL contains a bare carriage return".into(),
        );
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeProcessorOutputContract ACL is invalid: {error}"))?;
    let value = KnowledgeProcessorOutputContractV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeProcessorOutputContract ACL is not canonical".into());
    }
    Ok(value)
}
