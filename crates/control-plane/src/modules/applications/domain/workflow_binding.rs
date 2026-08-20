use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use serde::{Deserialize, Serialize};

/// Immutable, cross-context identity retained by an Application release.
///
/// Applications never copies a Workflow graph or payload. These identifiers
/// and digests are the complete evidence required to prove that a later
/// invocation resolves the same Workflow revision that publication admitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationWorkflowBinding {
    pub workflow_definition_id: WorkflowDefinitionId,
    pub workflow_revision_id: WorkflowRevisionId,
    pub workflow_contract_digest: Sha256Digest,
    pub workflow_payload_set_digest: Sha256Digest,
    pub workflow_semantic_contract_set_digest: Sha256Digest,
    pub input_schema_digest: Sha256Digest,
    pub output_schema_digest: Sha256Digest,
}

impl ApplicationWorkflowBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.workflow_definition_id.as_uuid().is_nil()
            || self.workflow_revision_id.as_uuid().is_nil()
        {
            return Err("Application Workflow binding identity is invalid".into());
        }
        for (label, digest) in [
            ("contract", &self.workflow_contract_digest),
            ("payload set", &self.workflow_payload_set_digest),
            (
                "semantic contract set",
                &self.workflow_semantic_contract_set_digest,
            ),
            ("input schema", &self.input_schema_digest),
            ("output schema", &self.output_schema_digest),
        ] {
            if Sha256Digest::parse(digest.as_str())? != *digest {
                return Err(format!(
                    "Application Workflow {label} digest is not canonical"
                ));
            }
        }
        Ok(())
    }

    pub fn validate_evidence(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        evidence: &ApplicationWorkflowRevisionEvidence,
    ) -> Result<(), String> {
        self.validate()?;
        evidence.validate()?;
        if evidence.organization_id != organization_id
            || evidence.project_id != project_id
            || evidence.binding != *self
        {
            return Err(
                "Application release does not match the exact admitted Workflow revision".into(),
            );
        }
        Ok(())
    }
}

/// Redacted admission evidence returned by the Workflow owning port.
///
/// The port returns metadata only. It never gives Applications a Workflow
/// graph, mutable head, plan, run history, or a right to write Workflow state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationWorkflowRevisionEvidence {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub binding: ApplicationWorkflowBinding,
}

impl ApplicationWorkflowRevisionEvidence {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.project_id.as_uuid().is_nil() {
            return Err("Application Workflow admission scope is invalid".into());
        }
        self.binding.validate()
    }
}
