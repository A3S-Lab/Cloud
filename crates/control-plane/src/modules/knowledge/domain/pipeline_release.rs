use super::acl::{
    exact_child, exact_shape, required_digest, required_digest_list, required_string,
    required_uuid, validate_name, validate_non_nil,
};
use super::types::{
    KnowledgeChunkStructureV1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
};
use crate::modules::shared_kernel::domain::{
    KnowledgePipelineId, KnowledgePipelineReleaseId, OrganizationId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_acl::builder::{list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Serialize};

const BLOCK: &str = "knowledge_pipeline_release";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePipelineReleaseSpecV1 {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub pipeline_id: KnowledgePipelineId,
    pub release_id: KnowledgePipelineReleaseId,
    pub name: String,
    pub chunk_structure: KnowledgeChunkStructureV1,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub workflow_revision_id: WorkflowRevisionId,
    pub workflow_revision_digest: Sha256Digest,
    pub datasource_entrance_digests: Vec<Sha256Digest>,
    pub output_contract_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePipelineReleaseV1 {
    spec: KnowledgePipelineReleaseSpecV1,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl KnowledgePipelineReleaseV1 {
    pub fn from_spec(mut spec: KnowledgePipelineReleaseSpecV1) -> Result<Self, String> {
        validate_non_nil(spec.organization_id.as_uuid(), "organization")?;
        validate_non_nil(spec.project_id.as_uuid(), "project")?;
        validate_non_nil(spec.pipeline_id.as_uuid(), "KnowledgePipeline")?;
        validate_non_nil(spec.release_id.as_uuid(), "KnowledgePipelineRelease")?;
        validate_non_nil(spec.workflow_definition_id.as_uuid(), "WorkflowDefinition")?;
        validate_non_nil(spec.workflow_revision_id.as_uuid(), "WorkflowRevision")?;
        validate_name(&spec.name)?;
        if spec.datasource_entrance_digests.is_empty()
            || spec.datasource_entrance_digests.len() > 32
        {
            return Err("KnowledgePipelineRelease datasource entrances must be bounded".into());
        }
        let mut unique = spec.datasource_entrance_digests.clone();
        unique.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        unique.dedup_by(|left, right| left.as_str() == right.as_str());
        if unique.len() != spec.datasource_entrance_digests.len() {
            return Err("KnowledgePipelineRelease datasource entrances must be unique".into());
        }
        spec.datasource_entrance_digests = unique;
        Sha256Digest::parse(spec.workflow_revision_digest.as_str())?;
        Sha256Digest::parse(spec.output_contract_digest.as_str())?;
        let workflow = BlockBuilder::new("workflow")
            .attr(
                "workflow_definition_id",
                string(&spec.workflow_definition_id.to_string()),
            )
            .attr(
                "workflow_revision_digest",
                string(spec.workflow_revision_digest.as_str()),
            )
            .attr(
                "workflow_revision_id",
                string(&spec.workflow_revision_id.to_string()),
            )
            .build();
        let document = Document {
            blocks: vec![BlockBuilder::new(BLOCK)
                .attr("chunk_structure", string(spec.chunk_structure.as_str()))
                .attr(
                    "datasource_entrance_digests",
                    list(
                        spec.datasource_entrance_digests
                            .iter()
                            .map(|digest| string(digest.as_str()))
                            .collect(),
                    ),
                )
                .attr("name", string(&spec.name))
                .attr(
                    "organization_id",
                    string(&spec.organization_id.to_string()),
                )
                .attr(
                    "output_contract_digest",
                    string(spec.output_contract_digest.as_str()),
                )
                .attr("pipeline_id", string(&spec.pipeline_id.to_string()))
                .attr("project_id", string(&spec.project_id.to_string()))
                .attr("release_id", string(&spec.release_id.to_string()))
                .attr("schema", string(KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1))
                .nested_block(workflow)
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
            return Err("stored KnowledgePipelineRelease ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &KnowledgePipelineReleaseSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(self.canonical_acl(), self.digest.as_str())?;
        if restored != *self {
            return Err("KnowledgePipelineRelease drifted from canonical ACL".into());
        }
        Ok(())
    }
}

fn parse_spec(document: &Document) -> Result<KnowledgePipelineReleaseSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("KnowledgePipelineRelease must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        BLOCK,
        &[
            "chunk_structure",
            "datasource_entrance_digests",
            "name",
            "organization_id",
            "output_contract_digest",
            "pipeline_id",
            "project_id",
            "release_id",
            "schema",
        ],
        &["workflow"],
    )?;
    if required_string(root, "schema")? != KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1 {
        return Err("KnowledgePipelineRelease schema is unsupported".into());
    }
    let workflow = exact_child(root, "workflow")?;
    exact_shape(
        workflow,
        "workflow",
        &[
            "workflow_definition_id",
            "workflow_revision_digest",
            "workflow_revision_id",
        ],
        &[],
    )?;
    Ok(KnowledgePipelineReleaseSpecV1 {
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        pipeline_id: KnowledgePipelineId::from_uuid(required_uuid(root, "pipeline_id")?),
        release_id: KnowledgePipelineReleaseId::from_uuid(required_uuid(root, "release_id")?),
        name: required_string(root, "name")?,
        chunk_structure: KnowledgeChunkStructureV1::parse(&required_string(
            root,
            "chunk_structure",
        )?)?,
        workflow_definition_id: WorkflowDefinitionId::from_uuid(required_uuid(
            workflow,
            "workflow_definition_id",
        )?),
        workflow_revision_id: WorkflowRevisionId::from_uuid(required_uuid(
            workflow,
            "workflow_revision_id",
        )?),
        workflow_revision_digest: required_digest(workflow, "workflow_revision_digest")?,
        datasource_entrance_digests: required_digest_list(root, "datasource_entrance_digests")?,
        output_contract_digest: required_digest(root, "output_contract_digest")?,
    })
}

fn seal(
    spec: KnowledgePipelineReleaseSpecV1,
    document: Document,
) -> Result<KnowledgePipelineReleaseV1, String> {
    let canonical_acl = format!("{}\n", generate_acl(&document));
    if canonical_acl.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgePipelineRelease ACL exceeds its storage bound".into());
    }
    let reparsed = parse_acl(&canonical_acl)
        .map_err(|error| format!("generated KnowledgePipelineRelease ACL is invalid: {error}"))?;
    let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
        format!("KnowledgePipelineRelease contract is not canonicalizable: {error}")
    })?)?;
    Ok(KnowledgePipelineReleaseV1 {
        spec,
        canonical_acl,
        digest,
    })
}

fn parse_sealed(source: &str) -> Result<KnowledgePipelineReleaseV1, String> {
    if source.is_empty() || source.len() > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES {
        return Err("KnowledgePipelineRelease ACL size is invalid".into());
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err("KnowledgePipelineRelease ACL contains a bare carriage return".into());
    }
    let normalized = source.replace("\r\n", "\n");
    let document = parse_acl(&normalized)
        .map_err(|error| format!("KnowledgePipelineRelease ACL is invalid: {error}"))?;
    let value = KnowledgePipelineReleaseV1::from_spec(parse_spec(&document)?)?;
    if value.canonical_acl != normalized {
        return Err("KnowledgePipelineRelease ACL is not canonical".into());
    }
    Ok(value)
}
