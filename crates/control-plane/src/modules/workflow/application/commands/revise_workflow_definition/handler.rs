use super::ReviseWorkflowDefinition;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, WorkflowRevisionId};
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::application::WorkflowDefinitionMutationResult;
use crate::modules::workflow::domain::{
    IWorkflowDefinitionRepository, ReviseWorkflowDefinitionWrite, WorkflowContract,
    WorkflowDefinitionRecord, WorkflowPayload, WorkflowRevision, WorkflowRevisionPublished,
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
            let revision = match WorkflowRevision::successor(
                &parent,
                WorkflowRevisionId::new(),
                contract.clone(),
                payloads,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if revision.contract.digest() == parent.contract.digest()
                && revision.payload_set_digest == parent.payload_set_digest
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
