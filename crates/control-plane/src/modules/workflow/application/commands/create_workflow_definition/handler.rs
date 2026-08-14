use super::CreateWorkflowDefinition;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::application::WorkflowDefinitionMutationResult;
use crate::modules::workflow::domain::{
    CreateWorkflowDefinitionWrite, IWorkflowDefinitionRepository, WorkflowCompositeRegions,
    WorkflowContract, WorkflowDefinition, WorkflowDefinitionRecord, WorkflowPayload,
    WorkflowRevision, WorkflowRevisionPublished, WorkflowRevisionSemanticContracts,
    WorkflowStepDescriptorBindings, WorkflowStepDescriptorRegistry, WorkflowVariableContract,
    WorkflowVariableDefaults,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateWorkflowDefinitionHandler {
    projects: Arc<dyn IProjectRepository>,
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
}

impl CreateWorkflowDefinitionHandler {
    pub fn new(
        projects: Arc<dyn IProjectRepository>,
        workflows: Arc<dyn IWorkflowDefinitionRepository>,
    ) -> Self {
        Self {
            projects,
            workflows,
        }
    }
}

impl CommandHandler<CreateWorkflowDefinition> for CreateWorkflowDefinitionHandler {
    fn execute(
        &self,
        command: CreateWorkflowDefinition,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<WorkflowDefinitionMutationResult>>,
    > {
        let projects = Arc::clone(&self.projects);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            match projects
                .find(command.organization_id, command.project_id)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => return Ok(Err(ApplicationError::NotFound("project not found".into()))),
                Err(error) => return Ok(Err(error.into())),
            }
            let contract = match WorkflowContract::parse_acl(&command.definition_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let payloads = match command
                .payloads
                .into_iter()
                .map(|value| WorkflowPayload::parse_acl(value.kind, &value.acl))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let now = Utc::now();
            let definition_id = WorkflowDefinitionId::new();
            let semantic_contracts = match command.semantic_contracts {
                Some(value) => match parse_semantic_contracts(&contract, value) {
                    Ok(value) => Some(value),
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                None => None,
            };
            let revision_id = WorkflowRevisionId::new();
            let revision_result = match semantic_contracts {
                Some(semantic_contracts) => WorkflowRevision::initial_with_semantic_contracts(
                    command.organization_id,
                    command.project_id,
                    definition_id,
                    revision_id,
                    contract.clone(),
                    payloads,
                    semantic_contracts,
                    command.actor_principal_id,
                    now,
                ),
                None => WorkflowRevision::initial(
                    command.organization_id,
                    command.project_id,
                    definition_id,
                    revision_id,
                    contract.clone(),
                    payloads,
                    command.actor_principal_id,
                    now,
                ),
            };
            let revision = match revision_result {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "contentDigest": contract.digest(),
                "payloadSetDigest": revision.payload_set_digest,
                "semanticContractSetDigest": revision.semantic_contract_set_digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/workflow-definitions",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let definition = match WorkflowDefinition::create(
                command.organization_id,
                command.project_id,
                definition_id,
                contract.spec().name.clone(),
                contract.spec().description.clone(),
                revision.id,
                contract.digest().clone(),
                command.actor_principal_id,
                revision.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event =
                WorkflowRevisionPublished::created(&definition, &revision, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match workflows
                .create(CreateWorkflowDefinitionWrite {
                    record: WorkflowDefinitionRecord {
                        definition,
                        revision,
                    },
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(WorkflowDefinitionMutationResult {
                record: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

fn parse_semantic_contracts(
    contract: &WorkflowContract,
    value: crate::modules::workflow::application::WorkflowSemanticContractAcls,
) -> Result<WorkflowRevisionSemanticContracts, String> {
    WorkflowRevisionSemanticContracts::create_with_optional_contracts(
        contract.spec(),
        WorkflowStepDescriptorBindings::parse_acl(&value.descriptor_bindings_acl)?,
        WorkflowStepDescriptorRegistry::parse_acl(&value.descriptor_registry_acl)?,
        WorkflowVariableContract::parse_acl(&value.variable_contract_acl)?,
        value
            .variable_defaults_acl
            .as_deref()
            .map(WorkflowVariableDefaults::parse_acl)
            .transpose()?,
        value
            .composite_regions_acl
            .as_deref()
            .map(WorkflowCompositeRegions::parse_acl)
            .transpose()?,
    )
}
