use crate::modules::applications::application::IApplicationWorkflowRevisionPort;
use crate::modules::applications::domain::{
    ApplicationWorkflowBinding, ApplicationWorkflowRevisionEvidence,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectId, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{IWorkflowDefinitionRepository, WorkflowStepKind};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Clone)]
pub struct WorkflowApplicationReleaseEvidenceReader {
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
}

impl WorkflowApplicationReleaseEvidenceReader {
    pub fn new(workflows: Arc<dyn IWorkflowDefinitionRepository>) -> Self {
        Self { workflows }
    }
}

#[async_trait]
impl IApplicationWorkflowRevisionPort for WorkflowApplicationReleaseEvidenceReader {
    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
        workflow_revision_id: WorkflowRevisionId,
    ) -> ApplicationResult<ApplicationWorkflowRevisionEvidence> {
        let revision = self
            .workflows
            .find_revision(
                organization_id,
                workflow_definition_id,
                workflow_revision_id,
            )
            .await?
            .filter(|revision| revision.project_id == project_id)
            .ok_or_else(|| ApplicationError::NotFound("Workflow revision not found".into()))?;
        revision.validate().map_err(|error| {
            ApplicationError::Internal(format!("stored Workflow revision is invalid: {error}"))
        })?;
        let semantic_contract_set_digest = revision
            .semantic_contract_set_digest()
            .cloned()
            .ok_or_else(|| {
                ApplicationError::Invalid(
                    "Application publication requires Workflow semantic-contract authority".into(),
                )
            })?;
        let input = revision
            .contract
            .spec()
            .steps
            .iter()
            .find(|step| step.kind == WorkflowStepKind::Input)
            .ok_or_else(|| {
                ApplicationError::Internal("stored Workflow input step is missing".into())
            })?;
        let outputs = revision
            .contract
            .spec()
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Output)
            .collect::<Vec<_>>();
        let [output] = outputs.as_slice() else {
            return Err(ApplicationError::Invalid(
                "Application publication requires exactly one Workflow Output step".into(),
            ));
        };
        let evidence = ApplicationWorkflowRevisionEvidence {
            organization_id,
            project_id,
            binding: ApplicationWorkflowBinding {
                workflow_definition_id,
                workflow_revision_id,
                workflow_contract_digest: revision.contract.digest().clone(),
                workflow_payload_set_digest: revision.payload_set_digest.clone(),
                workflow_semantic_contract_set_digest: semantic_contract_set_digest,
                input_schema_digest: input.output_schema_digest.clone(),
                output_schema_digest: output.output_schema_digest.clone(),
            },
        };
        evidence.validate().map_err(ApplicationError::Internal)?;
        Ok(evidence)
    }
}
