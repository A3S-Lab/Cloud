use super::resource_access::authorize_environment;
use super::{
    DeveloperWorkflowAccess, DeveloperWorkflowEnvironmentScope, IDeveloperWorkflowEnvironmentPort,
    IPreviewSourceSubscriptionQueryPort, PreviewSourceSubscriptionBinding,
};
use crate::modules::developer_workflows::domain::{
    AcceptPullRequestPreviewPolicyRevisionWrite, AcceptedPullRequestPreviewPolicyRevision,
    IPullRequestPreviewPolicyRepository, PullRequestPreviewPolicyContract,
    PullRequestPreviewPolicyRevisionAccepted, MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER,
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
    pub access: DeveloperWorkflowAccess,
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
    environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
}

impl AcceptPullRequestPreviewPolicyHandler {
    pub fn new(
        policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
        subscriptions: Arc<dyn IPreviewSourceSubscriptionQueryPort>,
        environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
    ) -> Self {
        Self {
            policies,
            subscriptions,
            environments,
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
        let environments = Arc::clone(&self.environments);
        Box::pin(async move {
            if let Err(error) = authorize_environment(
                environments.as_ref(),
                DeveloperWorkflowEnvironmentScope {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    environment_id: command.source_environment_id,
                },
                &command.access,
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
                    Some(value) if value <= MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER => value,
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
    revision.validate().is_ok()
        && revision.organization_id == command.organization_id
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

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/p0.3/pull-request-preview-policy.acl"
    ));

    #[test]
    fn acceptance_replay_fails_closed_on_corrupted_restored_revision() {
        let contract =
            PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE).expect("policy fixture");
        let policy = contract.policy();
        let environment_id = EnvironmentId::new();
        let actor_principal_id = PrincipalId::new();
        let command = AcceptPullRequestPreviewPolicy {
            organization_id: policy.organization_id,
            project_id: policy.project_id,
            source_environment_id: environment_id,
            source_subscription_id: policy.source_subscription_id,
            policy_acl: contract.canonical_acl().into(),
            access: DeveloperWorkflowAccess::organization_wide(),
            actor_principal_id,
            idempotency_key: "corrupt-replay".into(),
            request_id: Uuid::now_v7(),
        };
        let mut revision = AcceptedPullRequestPreviewPolicyRevision::accept(
            environment_id,
            contract.clone(),
            1,
            actor_principal_id,
            Utc::now(),
        )
        .expect("accepted revision");
        assert!(replay_matches(&revision, &command, &contract));

        revision.revision_number = MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER + 1;
        assert!(!replay_matches(&revision, &command, &contract));
    }
}
