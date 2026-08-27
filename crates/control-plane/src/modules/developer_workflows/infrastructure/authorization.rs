use crate::modules::developer_workflows::application::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    IDeveloperWorkflowAuthorizationPort,
};
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IResourceGrantRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Resolves one action-scoped Developer Workflows decision through the existing
/// Identity membership/grant authority and Projects environment authority.
#[derive(Clone)]
pub struct IdentityProjectsDeveloperWorkflowAuthorizationAdapter {
    memberships: Arc<dyn IMembershipRepository>,
    resource_grants: Arc<dyn IResourceGrantRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
}

impl IdentityProjectsDeveloperWorkflowAuthorizationAdapter {
    pub fn new(
        memberships: Arc<dyn IMembershipRepository>,
        resource_grants: Arc<dyn IResourceGrantRepository>,
        environments: Arc<dyn IEnvironmentRepository>,
    ) -> Self {
        Self {
            memberships,
            resource_grants,
            environments,
        }
    }
}

#[async_trait]
impl IDeveloperWorkflowAuthorizationPort for IdentityProjectsDeveloperWorkflowAuthorizationAdapter {
    async fn is_environment_action_allowed(
        &self,
        access: DeveloperWorkflowEnvironmentAccess,
    ) -> Result<bool, RepositoryError> {
        access.validate().map_err(RepositoryError::Forbidden)?;
        match access.action {
            DeveloperWorkflowAction::DetectBuildPlan
            | DeveloperWorkflowAction::ReadBuildPlan
            | DeveloperWorkflowAction::AcceptBuildPlan
            | DeveloperWorkflowAction::ReadWorkloadProfile
            | DeveloperWorkflowAction::AcceptWorkloadProfile
            | DeveloperWorkflowAction::ReadPullRequestPreviewPolicy
            | DeveloperWorkflowAction::AcceptPullRequestPreviewPolicy
            | DeveloperWorkflowAction::ReadPullRequestPreview => {}
        }

        let Some(membership) = self
            .memberships
            .find_active_membership_by_principal(access.organization_id, access.principal_id)
            .await?
        else {
            return Ok(false);
        };
        if !membership.is_active()
            || membership.organization_id != access.organization_id
            || membership.principal_id != access.principal_id
        {
            return Err(RepositoryError::Storage(
                "Identity returned inconsistent Developer Workflow membership evidence".into(),
            ));
        }

        let scopes = if membership.role == MembershipRole::Restricted {
            let grants = self
                .resource_grants
                .list_active_resource_grants_for_membership(access.organization_id, membership.id)
                .await?;
            if grants.iter().any(|grant| {
                !grant.is_active()
                    || grant.organization_id != access.organization_id
                    || grant.membership_id != membership.id
            }) {
                return Err(RepositoryError::Storage(
                    "Identity returned inconsistent Developer Workflow grant evidence".into(),
                ));
            }
            grants
                .into_iter()
                .map(|grant| grant.scope)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let evaluator = ResourceAccessEvaluator::for_membership(membership.role, scopes);
        if !evaluator.environment_is_visible(access.project_id, access.environment_id) {
            return Ok(false);
        }

        match self
            .environments
            .find(
                access.organization_id,
                access.project_id,
                access.environment_id,
            )
            .await?
        {
            Some(environment)
                if environment.organization_id == access.organization_id
                    && environment.project_id == access.project_id
                    && environment.id == access.environment_id
                    && environment.aggregate_version > 0 =>
            {
                Ok(true)
            }
            Some(_) => Err(RepositoryError::Storage(
                "Projects returned inconsistent Developer Workflow environment evidence".into(),
            )),
            None => Ok(false),
        }
    }
}
