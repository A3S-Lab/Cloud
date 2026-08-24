use super::ReviseWorkflowDefinition;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, WorkflowRevisionId};
use crate::modules::workflow::application::{resource_access, WorkflowDefinitionMutationResult};
use crate::modules::workflow::domain::{
    IWorkflowDefinitionRepository, ReviseWorkflowDefinitionWrite, WorkflowCompositeRegions,
    WorkflowContract, WorkflowDefinitionRecord, WorkflowPayload, WorkflowRevision,
    WorkflowRevisionPublished, WorkflowRevisionSemanticContracts, WorkflowStepDescriptorBindings,
    WorkflowStepDescriptorRegistry, WorkflowVariableContract, WorkflowVariableDefaults,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct ReviseWorkflowDefinitionHandler {
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
}

impl ReviseWorkflowDefinitionHandler {
    pub fn new(workflows: Arc<dyn IWorkflowDefinitionRepository>) -> Self {
        Self { workflows }
    }
}

impl CommandHandler<ReviseWorkflowDefinition> for ReviseWorkflowDefinitionHandler {
    fn execute(
        &self,
        command: ReviseWorkflowDefinition,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<WorkflowDefinitionMutationResult>>,
    > {
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected WorkflowDefinition version must be positive".into(),
                )));
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
            let current = match resource_access::workflow_definition(
                workflows.as_ref(),
                command.organization_id,
                command.workflow_definition_id,
                &command.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let parent = match workflows
                .list_revisions(command.organization_id, command.workflow_definition_id)
                .await
            {
                Ok(values) => values
                    .into_iter()
                    .find(|revision| revision.revision_number == command.expected_version),
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(parent) = parent else {
                return Ok(Err(ApplicationError::Conflict(
                    "expected WorkflowDefinition version is not available in this lineage".into(),
                )));
            };
            let semantic_contracts = match command.semantic_contracts {
                Some(value) => match parse_semantic_contracts(&contract, value) {
                    Ok(value) => Some(value),
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                None => None,
            };
            let revision_id = WorkflowRevisionId::new();
            let revision_result = match semantic_contracts {
                Some(semantic_contracts) => WorkflowRevision::successor_with_semantic_contracts(
                    &parent,
                    revision_id,
                    contract.clone(),
                    payloads,
                    semantic_contracts,
                    command.actor_principal_id,
                    Utc::now(),
                ),
                None => WorkflowRevision::successor(
                    &parent,
                    revision_id,
                    contract.clone(),
                    payloads,
                    command.actor_principal_id,
                    Utc::now(),
                ),
            };
            let revision = match revision_result {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = revision.validate_runtime_dispatch_support() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            if revision.contract.digest() == parent.contract.digest()
                && revision.payload_set_digest == parent.payload_set_digest
                && revision.semantic_contract_set_digest() == parent.semantic_contract_set_digest()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "WorkflowDefinition revision must change semantic content".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "workflowDefinitionId": command.workflow_definition_id,
                "expectedVersion": command.expected_version,
                "contentDigest": contract.digest(),
                "payloadSetDigest": revision.payload_set_digest,
                "semanticContractSetDigest": revision.semantic_contract_set_digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/workflow-definitions/{}/revisions",
                    command.organization_id, command.workflow_definition_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let base = current.at_revision(&parent).map_err(|error| {
                BootError::Internal(format!("stored Workflow revision is invalid: {error}"))
            })?;
            let definition = match base.advance(
                command.expected_version,
                contract.spec().name.clone(),
                contract.spec().description.clone(),
                revision.id,
                contract.digest().clone(),
                revision.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event =
                WorkflowRevisionPublished::revised(&definition, &revision, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match workflows
                .revise(ReviseWorkflowDefinitionWrite {
                    record: WorkflowDefinitionRecord {
                        definition,
                        revision,
                    },
                    expected_version: command.expected_version,
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
