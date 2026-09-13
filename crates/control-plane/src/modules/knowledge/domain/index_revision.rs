use super::acl::{
    exact_shape, required_digest, required_string, required_string_list, required_u32,
    required_uuid, validate_non_nil, validate_tags,
};
use super::types::{
    KnowledgeIndexStrategyV1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_INDEX_REVISION_SCHEMA_V1,
    KNOWLEDGE_MAX_EMBEDDING_DIMENSION,
};
use crate::modules::shared_kernel::domain::{
    KnowledgeBaseRevisionId, KnowledgeIndexRevisionId, OrganizationId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{list, number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_index_revision";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeIndexRevisionSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub index_revision_id: KnowledgeIndexRevisionId,
    pub strategy: KnowledgeIndexStrategyV1,
    pub embedding_model_revision_digest: Sha256Digest,
    pub embedding_dimension: u32,
    pub input_modalities: Vec<String>,
    pub retrieval_modalities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeIndexRevisionV1 {
    spec: KnowledgeIndexRevisionSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgeIndexRevisionV1 {
    pub fn from_spec(mut spec: KnowledgeIndexRevisionSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(
            spec.knowledge_base_revision_id.as_uuid(),
            "KnowledgeBaseRevision",
        )?;
        validate_non_nil(spec.index_revision_id.as_uuid(), "KnowledgeIndexRevision")?;
        if matches!(
            spec.strategy,
            KnowledgeIndexStrategyV1::Vector | KnowledgeIndexStrategyV1::Hybrid
        ) && !(1..=KNOWLEDGE_MAX_EMBEDDING_DIMENSION).contains(&spec.embedding_dimension)
        {
            return Err("KnowledgeIndexRevision embedding dimension is out of bounds".into());
        }
        if matches!(
            spec.strategy,
            KnowledgeIndexStrategyV1::FullText | KnowledgeIndexStrategyV1::Inverted
        ) && spec.embedding_dimension != 0
        {
            return Err("non-vector KnowledgeIndexRevision must use embedding_dimension 0".into());
        }
        validate_modality_list(&mut spec.input_modalities, "input")?;
        validate_modality_list(&mut spec.retrieval_modalities, "retrieval")?;
        Sha256Digest::parse(spec.embedding_model_revision_digest.as_str())?;
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr(
                    "embedding_dimension",
                    number(spec.embedding_dimension as f64),
                )
                .attr(
                    "embedding_model_revision_digest",
                    string(spec.embedding_model_revision_digest.as_str()),
                )
                .attr(
                    "index_revision_id",
                    string(&spec.index_revision_id.to_string()),
                )
                .attr(
                    "input_modalities",
                    list(spec.input_modalities.iter().map(|value| string(value.as_str())).collect()),
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
                    "retrieval_modalities",
                    list(
                        spec.retrieval_modalities
                            .iter()
                            .cloned()
                            .map(|value| string(value.as_str()))
                            .collect(),
                    ),
                )
                .attr("schema", string(KNOWLEDGE_INDEX_REVISION_SCHEMA_V1))
                .attr("strategy", string(spec.strategy.as_str()))
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
            return Err("stored KnowledgeIndexRevision ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgeIndexRevisionSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_modality_list(values: &mut Vec<String>, label: &str) -> Result<(), String> {
    if values.is_empty() || values.len() > 8 {
        return Err(format!("Knowledge {label} modalities must be bounded and non-empty"));
    }
    values.sort();
    values.dedup();
    validate_tags(values)
}

fn parse_spec(document: &Document) -> Result<KnowledgeIndexRevisionSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgeIndexRevision must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "embedding_dimension",
            "embedding_model_revision_digest",
            "index_revision_id",
            "input_modalities",
            "knowledge_base_revision_id",
            "organization_id",
            "project_id",
            "retrieval_modalities",
            "schema",
            "strategy",
        ],
        &[],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_INDEX_REVISION_SCHEMA_V1 {
        return Err("KnowledgeIndexRevision schema is unsupported".into());
    }
    Ok(KnowledgeIndexRevisionSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(required_uuid(
            root,
            "knowledge_base_revision_id",
        )?),
        index_revision_id: KnowledgeIndexRevisionId::from_uuid(required_uuid(
            root,
            "index_revision_id",
        )?),
        strategy: KnowledgeIndexStrategyV1::parse(&required_string(root, "strategy")?)?,
        embedding_model_revision_digest: required_digest(
            root,
            "embedding_model_revision_digest",
        )?,
        embedding_dimension: required_u32(
            root,
            "embedding_dimension",
            0,
            KNOWLEDGE_MAX_EMBEDDING_DIMENSION,
        )?,
        input_modalities: required_string_list(root, "input_modalities")?,
        retrieval_modalities: required_string_list(root, "retrieval_modalities")?,
    })
}

fn seal(
    spec: KnowledgeIndexRevisionSpecV1,
    document: Document,
) -> Result<KnowledgeIndexRevisionV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeIndexRevision ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl)
        .map_err(|error| format!("generated KnowledgeIndexRevision ACL is invalid: {error}"))?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgeIndexRevision contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgeIndexRevisionV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgeIndexRevisionV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgeIndexRevision ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgeIndexRevision ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgeIndexRevision ACL is invalid: {error}"))?;
    let value = KnowledgeIndexRevisionV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgeIndexRevision ACL is not canonical".into());
    }
    Ok(value)
}
