use super::authorization::authorize_environment_action;
use super::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess, IBuildPlanSourceRevisionPort,
    IDeveloperWorkflowAuthorizationPort,
};
use crate::modules::developer_workflows::domain::{
    AcceptBuildPlanWrite, AcceptedBuildPlan, AcceptedBuildPlanContract, BuildPlanAccepted,
    BuildPlanProposal, IBuildPlanRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptBuildPlan {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub proposal_acl: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AcceptBuildPlan {
    type Output = ApplicationResult<AcceptBuildPlanResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptBuildPlanResult {
    pub plan: AcceptedBuildPlan,
    pub replayed: bool,
}

pub struct AcceptBuildPlanHandler {
    plans: Arc<dyn IBuildPlanRepository>,
    sources: Arc<dyn IBuildPlanSourceRevisionPort>,
    authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
}

impl AcceptBuildPlanHandler {
    pub fn new(
        plans: Arc<dyn IBuildPlanRepository>,
        sources: Arc<dyn IBuildPlanSourceRevisionPort>,
        authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
    ) -> Self {
        Self {
            plans,
            sources,
            authorization,
        }
    }
}

impl CommandHandler<AcceptBuildPlan> for AcceptBuildPlanHandler {
    fn execute(
        &self,
        command: AcceptBuildPlan,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AcceptBuildPlanResult>>>
    {
        let plans = Arc::clone(&self.plans);
        let sources = Arc::clone(&self.sources);
        let authorization = Arc::clone(&self.authorization);
        Box::pin(async move {
            if let Err(error) = authorize_environment_action(
                authorization.as_ref(),
                DeveloperWorkflowEnvironmentAccess {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    environment_id: command.environment_id,
                    principal_id: command.actor_principal_id,
                    action: DeveloperWorkflowAction::AcceptBuildPlan,
                },
            )
            .await
            {
                return Ok(Err(error));
            }
            let proposal = match BuildPlanProposal::parse_acl(&command.proposal_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let contract = match AcceptedBuildPlanContract::from_proposal(
                command.source_revision_id,
                proposal,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&CanonicalAcceptance {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                source_revision_id: command.source_revision_id,
                plan_digest: contract.digest().as_str(),
                actor_principal_id: command.actor_principal_id,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/build-plans",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match plans.replay_acceptance(&idempotency).await {
                Ok(Some(plan)) => {
                    if !replay_matches(
                        &plan,
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        command.source_revision_id,
                        contract.digest().as_str(),
                    ) {
                        return Err(BootError::Internal(
                            "accepted BuildPlan replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(AcceptBuildPlanResult {
                        plan,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let source = match sources
                .resolve(command.organization_id, command.source_revision_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "BuildPlan Source revision not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let plan = match AcceptedBuildPlan::accept(
                command.organization_id,
                command.project_id,
                command.environment_id,
                contract,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = source.validate_binding(&plan) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event = BuildPlanAccepted::envelope(&plan, command.request_id)
                .map_err(BootError::Internal)?;
            match plans
                .accept(AcceptBuildPlanWrite {
                    plan,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(AcceptBuildPlanResult {
                    plan: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn replay_matches(
    plan: &AcceptedBuildPlan,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    plan_digest: &str,
) -> bool {
    plan.organization_id == organization_id
        && plan.project_id == project_id
        && plan.environment_id == environment_id
        && plan.source_revision_id == source_revision_id
        && plan.contract.digest().as_str() == plan_digest
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalAcceptance<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    plan_digest: &'a str,
    actor_principal_id: PrincipalId,
}
