use crate::modules::developer_workflows::domain::{
    AcceptWorkloadProfileRevisionWrite, AcceptedWorkloadProfileRevision, IBuildPlanRepository,
    IWorkloadProfileRepository, WorkloadProfileContract, WorkloadProfileRevisionAccepted,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
    RepositoryError,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptWorkloadProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub profile_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AcceptWorkloadProfile {
    type Output = ApplicationResult<AcceptWorkloadProfileResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptWorkloadProfileResult {
    pub revision: AcceptedWorkloadProfileRevision,
    pub replayed: bool,
}

pub struct AcceptWorkloadProfileHandler {
    profiles: Arc<dyn IWorkloadProfileRepository>,
    plans: Arc<dyn IBuildPlanRepository>,
}

impl AcceptWorkloadProfileHandler {
    pub fn new(
        profiles: Arc<dyn IWorkloadProfileRepository>,
        plans: Arc<dyn IBuildPlanRepository>,
    ) -> Self {
        Self { profiles, plans }
    }
}

impl CommandHandler<AcceptWorkloadProfile> for AcceptWorkloadProfileHandler {
    fn execute(
        &self,
        command: AcceptWorkloadProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptWorkloadProfileResult>>,
    > {
        let profiles = Arc::clone(&self.profiles);
        let plans = Arc::clone(&self.plans);
        Box::pin(async move {
            if !command
                .resource_access
                .allows(ResourceGrantScope::Environment {
                    project_id: command.project_id,
                    environment_id: command.environment_id,
                })
            {
                return Ok(Err(ApplicationError::NotFound(
                    "workload profile environment not found".into(),
                )));
            }
            let contract = match WorkloadProfileContract::parse_acl(&command.profile_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if contract.spec().build_plan_id != command.build_plan_id {
                return Ok(Err(ApplicationError::Invalid(
                    "workload profile command changed its embedded BuildPlan identity".into(),
                )));
            }
            let canonical = serde_json::to_vec(&CanonicalAcceptance {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                build_plan_id: command.build_plan_id,
                profile_digest: contract.digest().as_str(),
                actor_principal_id: command.actor_principal_id,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/workload-profiles",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key.clone(),
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match profiles.replay_acceptance(&idempotency).await {
                Ok(Some(revision)) => {
                    if !replay_matches(&revision, &command, &contract) {
                        return Err(BootError::Internal(
                            "workload profile acceptance replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(AcceptWorkloadProfileResult {
                        revision,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let build_plan = match plans
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.build_plan_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "workload profile BuildPlan not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = contract.validate_for(&build_plan) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let profile_id = match AcceptedWorkloadProfileRevision::profile_id_for(
                command.organization_id,
                command.project_id,
                command.environment_id,
                &contract,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let previous = match profiles
                .find_current(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    profile_id,
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
                            "workload profile revision number exhausted".into(),
                        )))
                    }
                },
                None => 1,
            };
            let revision = match AcceptedWorkloadProfileRevision::accept(
                &build_plan,
                contract,
                revision_number,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = WorkloadProfileRevisionAccepted::envelope(&revision, command.request_id)
                .map_err(BootError::Internal)?;
            match profiles
                .accept(AcceptWorkloadProfileRevisionWrite {
                    revision,
                    build_plan,
                    expected_previous_revision_id: previous.map(|value| value.id),
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(AcceptWorkloadProfileResult {
                    revision: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

fn replay_matches(
    revision: &AcceptedWorkloadProfileRevision,
    command: &AcceptWorkloadProfile,
    contract: &WorkloadProfileContract,
) -> bool {
    revision.organization_id == command.organization_id
        && revision.project_id == command.project_id
        && revision.environment_id == command.environment_id
        && revision.build_plan_id == command.build_plan_id
        && revision.contract.digest() == contract.digest()
        && revision.accepted_by == command.actor_principal_id
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalAcceptance<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    build_plan_id: BuildPlanId,
    profile_digest: &'a str,
    actor_principal_id: PrincipalId,
}
