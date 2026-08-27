use super::authorization::authorize_environment_action;
use super::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    IDeveloperWorkflowAuthorizationPort, IPreviewSourceSubscriptionQueryPort,
    PreviewSourceSubscriptionBinding,
};
use crate::modules::developer_workflows::domain::{
    AcceptPullRequestPreviewPolicyRevisionWrite, AcceptedPullRequestPreviewPolicyRevision,
    IPullRequestPreviewPolicyRepository, PullRequestPreviewPolicyContract,
    PullRequestPreviewPolicyRevisionAccepted,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    SourceSubscriptionId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptPullRequestPreviewPolicy {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub policy_acl: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AcceptPullRequestPreviewPolicy {
    type Output = ApplicationResult<AcceptPullRequestPreviewPolicyResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptPullRequestPreviewPolicyResult {
    pub revision: AcceptedPullRequestPreviewPolicyRevision,
    pub replayed: bool,
}

pub struct AcceptPullRequestPreviewPolicyHandler {
    policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
    subscriptions: Arc<dyn IPreviewSourceSubscriptionQueryPort>,
    authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
}

impl AcceptPullRequestPreviewPolicyHandler {
    pub fn new(
        policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
        subscriptions: Arc<dyn IPreviewSourceSubscriptionQueryPort>,
        authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
    ) -> Self {
        Self {
            policies,
            subscriptions,
            authorization,
        }
    }
}

impl CommandHandler<AcceptPullRequestPreviewPolicy> for AcceptPullRequestPreviewPolicyHandler {
    fn execute(
        &self,
        command: AcceptPullRequestPreviewPolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptPullRequestPreviewPolicyResult>>,
    > {
        let policies = Arc::clone(&self.policies);
        let subscriptions = Arc::clone(&self.subscriptions);
        let authorization = Arc::clone(&self.authorization);
        Box::pin(async move {
            if let Err(error) = authorize_environment_action(
                authorization.as_ref(),
                DeveloperWorkflowEnvironmentAccess {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    environment_id: command.source_environment_id,
                    principal_id: command.actor_principal_id,
                    action: DeveloperWorkflowAction::AcceptPullRequestPreviewPolicy,
                },
            )
            .await
            {
                return Ok(Err(error));
            }
            let contract = match PullRequestPreviewPolicyContract::parse_acl(&command.policy_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if contract.policy().organization_id != command.organization_id
                || contract.policy().project_id != command.project_id
                || contract.policy().source_subscription_id != command.source_subscription_id
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Preview policy command changed its embedded owner binding".into(),
                )));
            }
            let canonical = serde_json::to_vec(&CanonicalAcceptance {
                organization_id: command.organization_id,
                project_id: command.project_id,
                source_environment_id: command.source_environment_id,
                source_subscription_id: command.source_subscription_id,
                policy_digest: contract.digest().as_str(),
                actor_principal_id: command.actor_principal_id,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/pull-request-preview-policies",
                    command.organization_id, command.project_id, command.source_environment_id
                ),
                command.idempotency_key.clone(),
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match policies.replay_acceptance(&idempotency).await {
                Ok(Some(revision)) => {
                    if !replay_matches(&revision, &command, &contract) {
                        return Err(BootError::Internal(
                            "Preview policy acceptance replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(AcceptPullRequestPreviewPolicyResult {
                        revision,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let source = match subscriptions
                .resolve(command.organization_id, command.source_subscription_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Preview source subscription not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = validate_source_binding(&source, &command, &contract) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let previous = match policies
                .find_current(
                    command.organization_id,
                    command.project_id,
                    command.source_environment_id,
                    command.source_subscription_id,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let revision_number = match previous.as_ref() {
                Some(value) => match value.revision_number.checked_add(1) {
                    Some(value) if value <= i64::MAX as u64 => value,
                    _ => {
                        return Ok(Err(ApplicationError::Conflict(
                            "Preview policy revision number exhausted".into(),
                        )))
                    }
                },
                None => 1,
            };
            let revision = match AcceptedPullRequestPreviewPolicyRevision::accept(
                command.source_environment_id,
                contract,
                revision_number,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event =
                PullRequestPreviewPolicyRevisionAccepted::envelope(&revision, command.request_id)
                    .map_err(BootError::Internal)?;
            match policies
                .accept(AcceptPullRequestPreviewPolicyRevisionWrite {
                    revision,
                    expected_previous_revision_id: previous.map(|value| value.id),
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(AcceptPullRequestPreviewPolicyResult {
                    revision: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

fn validate_source_binding(
    source: &PreviewSourceSubscriptionBinding,
    command: &AcceptPullRequestPreviewPolicy,
    contract: &PullRequestPreviewPolicyContract,
) -> Result<(), String> {
    source.validate()?;
    let policy = contract.policy();
    if !source.active
        || source.organization_id != command.organization_id
        || source.project_id != command.project_id
        || source.environment_id != command.source_environment_id
        || source.source_subscription_id != command.source_subscription_id
        || source.installation_id != policy.installation_id
        || source.repository != policy.base_repository
        || source.branch != policy.base_branch
    {
        return Err("Preview policy does not match the exact active source subscription".into());
    }
    Ok(())
}

fn replay_matches(
    revision: &AcceptedPullRequestPreviewPolicyRevision,
    command: &AcceptPullRequestPreviewPolicy,
    contract: &PullRequestPreviewPolicyContract,
) -> bool {
    revision.organization_id == command.organization_id
        && revision.project_id == command.project_id
        && revision.source_environment_id == command.source_environment_id
        && revision.source_subscription_id == command.source_subscription_id
        && revision.contract.digest() == contract.digest()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalAcceptance<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
    policy_digest: &'a str,
    actor_principal_id: PrincipalId,
}
