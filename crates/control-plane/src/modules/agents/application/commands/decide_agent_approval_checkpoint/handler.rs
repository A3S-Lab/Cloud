use super::{DecideAgentApprovalCheckpoint, DecideAgentApprovalCheckpointResult};
use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::application::support::{idempotency, validate_request_id};
use crate::modules::agents::application::{
    AgentApprovalAuthorization, IAgentApprovalAuthorizationPort,
};
use crate::modules::agents::domain::{
    DecideAgentApprovalCheckpointWrite, IAgentRepository, validate_agent_approval_reason,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentApprovalDecisionId, canonical_timestamp};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use a3s_cloud_contracts::AgentProviderApprovalOutcomeV1;
use std::sync::Arc;

pub struct DecideAgentApprovalCheckpointHandler {
    agents: Arc<dyn IAgentRepository>,
    authorization: Arc<dyn IAgentApprovalAuthorizationPort>,
}

impl DecideAgentApprovalCheckpointHandler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        authorization: Arc<dyn IAgentApprovalAuthorizationPort>,
    ) -> Self {
        Self {
            agents,
            authorization,
        }
    }
}

impl CommandHandler<DecideAgentApprovalCheckpoint> for DecideAgentApprovalCheckpointHandler {
    fn execute(
        &self,
        command: DecideAgentApprovalCheckpoint,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DecideAgentApprovalCheckpointResult>>,
    > {
        let agents = Arc::clone(&self.agents);
        let authorization = Arc::clone(&self.authorization);
        Box::pin(async move {
            if let Err(error) = validate_request_id(command.request_id) {
                return Ok(Err(error));
            }
            if command.expected_version == 0
                || !matches!(
                    command.outcome,
                    AgentProviderApprovalOutcomeV1::Approved
                        | AgentProviderApprovalOutcomeV1::Denied
                )
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent approval decision is invalid".into(),
                )));
            }
            if let Err(error) = validate_agent_approval_reason(command.reason.as_deref()) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let access = match AgentResourceAccess::new(Arc::clone(&agents))
                .execution(
                    command.organization_id,
                    command.execution_id,
                    &command.access,
                )
                .await
            {
                Ok(access) => access,
                Err(error) => return Ok(Err(error)),
            };
            let checkpoint = match agents
                .find_checkpoint(command.organization_id, command.checkpoint_id)
                .await
            {
                Ok(Some(checkpoint))
                    if checkpoint.execution_id == access.execution.id
                        && checkpoint.conversation_id == access.conversation.id =>
                {
                    checkpoint
                }
                Ok(Some(_))
                | Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Agent approval checkpoint not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/agent-executions/{}/approval-checkpoints/{}/decision",
                    command.organization_id, command.execution_id, command.checkpoint_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "organizationId": command.organization_id,
                    "executionId": command.execution_id,
                    "checkpointId": command.checkpoint_id,
                    "expectedVersion": command.expected_version,
                    "outcome": command.outcome,
                    "reason": command.reason,
                }),
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(error)),
            };
            match agents.replay_checkpoint_decision(&idempotency).await {
                Ok(Some(replay))
                    if replay.organization_id == command.organization_id
                        && replay.execution_id == command.execution_id
                        && replay.id == command.checkpoint_id =>
                {
                    return Ok(Ok(DecideAgentApprovalCheckpointResult {
                        checkpoint: replay,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "Agent approval replay changed its immutable identity".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let authorization_decision = match authorization
                .authorize_decision(AgentApprovalAuthorization {
                    organization_id: command.organization_id,
                    project_id: access.conversation.project_id,
                    environment_id: access.conversation.environment_id,
                    principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    request_id: command.request_id,
                })
                .await
            {
                Ok(reference) => reference,
                Err(error) => return Ok(Err(error)),
            };
            let decision_id = AgentApprovalDecisionId::from_uuid(uuid::Uuid::new_v5(
                &checkpoint.id.as_uuid(),
                format!(
                    "a3s-agent-approval-decision-v1:{}:{}",
                    idempotency.key, idempotency.request_digest
                )
                .as_bytes(),
            ));
            match agents
                .decide_checkpoint(DecideAgentApprovalCheckpointWrite {
                    organization_id: command.organization_id,
                    checkpoint_id: checkpoint.id,
                    expected_version: command.expected_version,
                    decision_id,
                    outcome: command.outcome,
                    decided_by: command.actor_principal_id,
                    authorization_decision,
                    reason: command.reason,
                    decided_at: canonical_timestamp(
                        command.requested_at.max(checkpoint.updated_at),
                    ),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(DecideAgentApprovalCheckpointResult {
                    checkpoint: write.checkpoint,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
