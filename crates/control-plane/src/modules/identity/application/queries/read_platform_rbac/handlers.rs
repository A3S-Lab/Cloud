use super::{
    GetCurrentPlatformRolePolicy, GetPlatformRoleBinding, GetPlatformRolePolicyRevision,
    GetPrincipalPlatformRoleBinding,
};
use crate::modules::identity::application::privileged_management::{installation_id, not_found};
use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRoleBinding,
};
use crate::modules::identity::domain::repositories::{
    IIdentityBootstrapRepository, IPlatformRbacRepository, ReadCurrentPlatformRolePolicy,
    ReadPlatformRoleBinding, ReadPlatformRolePolicyRevision, ReadPrincipalPlatformRoleBinding,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetCurrentPlatformRolePolicyHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl GetCurrentPlatformRolePolicyHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl QueryHandler<GetCurrentPlatformRolePolicy> for GetCurrentPlatformRolePolicyHandler {
    fn execute(
        &self,
        query: GetCurrentPlatformRolePolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedPlatformRolePolicyRevision>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_current_platform_role_policy(ReadCurrentPlatformRolePolicy {
                    installation_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("current platform role policy"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetPlatformRolePolicyRevisionHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl GetPlatformRolePolicyRevisionHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl QueryHandler<GetPlatformRolePolicyRevision> for GetPlatformRolePolicyRevisionHandler {
    fn execute(
        &self,
        query: GetPlatformRolePolicyRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedPlatformRolePolicyRevision>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_platform_role_policy_revision(ReadPlatformRolePolicyRevision {
                    installation_id,
                    revision_id: query.revision_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("platform role policy revision"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetPlatformRoleBindingHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl GetPlatformRoleBindingHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl QueryHandler<GetPlatformRoleBinding> for GetPlatformRoleBindingHandler {
    fn execute(
        &self,
        query: GetPlatformRoleBinding,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PlatformRoleBinding>>>
    {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_platform_role_binding(ReadPlatformRoleBinding {
                    installation_id,
                    binding_id: query.binding_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("platform role binding"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetPrincipalPlatformRoleBindingHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl GetPrincipalPlatformRoleBindingHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl QueryHandler<GetPrincipalPlatformRoleBinding> for GetPrincipalPlatformRoleBindingHandler {
    fn execute(
        &self,
        query: GetPrincipalPlatformRoleBinding,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PlatformRoleBinding>>>
    {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_principal_platform_role_binding(ReadPrincipalPlatformRoleBinding {
                    installation_id,
                    principal_id: query.principal_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("active Principal platform role binding"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
