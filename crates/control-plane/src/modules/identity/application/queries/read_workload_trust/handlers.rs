use super::{
    GetCurrentTrustDomain, GetCurrentWorkloadIdentityPolicy,
    GetCurrentWorkloadIdentityPolicyForWorkload, GetTrustDomainRevision,
    GetWorkloadIdentityPolicyRevision, ListTrustDomainRevisions,
    ListWorkloadIdentityPolicyRevisions,
};
use crate::modules::identity::application::privileged_management::{installation_id, not_found};
use crate::modules::identity::domain::entities::{
    AcceptedTrustDomainRevision, AcceptedWorkloadIdentityPolicyRevision,
};
use crate::modules::identity::domain::repositories::{
    IIdentityBootstrapRepository, ITrustDomainRepository, IWorkloadIdentityPolicyRepository,
    ListTrustDomainRevisions as ListTrustDomainRevisionsRead,
    ListWorkloadIdentityPolicyRevisions as ListWorkloadIdentityPolicyRevisionsRead,
    ReadCurrentTrustDomain, ReadCurrentWorkloadIdentityPolicy,
    ReadCurrentWorkloadIdentityPolicyForWorkload, ReadTrustDomainRevision,
    ReadWorkloadIdentityPolicyRevision,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

macro_rules! workload_trust_handler {
    ($name:ident, $trait_name:ident) => {
        pub struct $name {
            bootstrap: Arc<dyn IIdentityBootstrapRepository>,
            repository: Arc<dyn $trait_name>,
        }

        impl $name {
            pub fn new(
                bootstrap: Arc<dyn IIdentityBootstrapRepository>,
                repository: Arc<dyn $trait_name>,
            ) -> Self {
                Self {
                    bootstrap,
                    repository,
                }
            }
        }
    };
}

workload_trust_handler!(GetCurrentTrustDomainHandler, ITrustDomainRepository);
workload_trust_handler!(GetTrustDomainRevisionHandler, ITrustDomainRepository);
workload_trust_handler!(ListTrustDomainRevisionsHandler, ITrustDomainRepository);
workload_trust_handler!(
    GetCurrentWorkloadIdentityPolicyHandler,
    IWorkloadIdentityPolicyRepository
);
workload_trust_handler!(
    GetCurrentWorkloadIdentityPolicyForWorkloadHandler,
    IWorkloadIdentityPolicyRepository
);
workload_trust_handler!(
    GetWorkloadIdentityPolicyRevisionHandler,
    IWorkloadIdentityPolicyRepository
);
workload_trust_handler!(
    ListWorkloadIdentityPolicyRevisionsHandler,
    IWorkloadIdentityPolicyRepository
);

impl QueryHandler<GetCurrentTrustDomain> for GetCurrentTrustDomainHandler {
    fn execute(
        &self,
        query: GetCurrentTrustDomain,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedTrustDomainRevision>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_current(ReadCurrentTrustDomain {
                    installation_id,
                    trust_domain_id: query.trust_domain_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("current trust-domain revision"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

impl QueryHandler<GetTrustDomainRevision> for GetTrustDomainRevisionHandler {
    fn execute(
        &self,
        query: GetTrustDomainRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedTrustDomainRevision>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_revision(ReadTrustDomainRevision {
                    installation_id,
                    trust_domain_id: query.trust_domain_id,
                    revision_id: query.revision_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("trust-domain revision"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

impl QueryHandler<ListTrustDomainRevisions> for ListTrustDomainRevisionsHandler {
    fn execute(
        &self,
        query: ListTrustDomainRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<AcceptedTrustDomainRevision>>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .list_revisions(ListTrustDomainRevisionsRead {
                    installation_id,
                    trust_domain_id: query.trust_domain_id,
                    limit: query.limit,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(value) => Ok(Ok(value)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

impl QueryHandler<GetCurrentWorkloadIdentityPolicy> for GetCurrentWorkloadIdentityPolicyHandler {
    fn execute(
        &self,
        query: GetCurrentWorkloadIdentityPolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedWorkloadIdentityPolicyRevision>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_current(ReadCurrentWorkloadIdentityPolicy {
                    installation_id,
                    organization_id: query.organization_id,
                    policy_id: query.policy_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("current workload identity policy revision"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

impl QueryHandler<GetCurrentWorkloadIdentityPolicyForWorkload>
    for GetCurrentWorkloadIdentityPolicyForWorkloadHandler
{
    fn execute(
        &self,
        query: GetCurrentWorkloadIdentityPolicyForWorkload,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedWorkloadIdentityPolicyRevision>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_current_for_workload(ReadCurrentWorkloadIdentityPolicyForWorkload {
                    installation_id,
                    organization_id: query.organization_id,
                    workload_id: query.workload_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("current Workload identity policy revision"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

impl QueryHandler<GetWorkloadIdentityPolicyRevision> for GetWorkloadIdentityPolicyRevisionHandler {
    fn execute(
        &self,
        query: GetWorkloadIdentityPolicyRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedWorkloadIdentityPolicyRevision>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_revision(ReadWorkloadIdentityPolicyRevision {
                    installation_id,
                    organization_id: query.organization_id,
                    policy_id: query.policy_id,
                    revision_id: query.revision_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("workload identity policy revision"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

impl QueryHandler<ListWorkloadIdentityPolicyRevisions>
    for ListWorkloadIdentityPolicyRevisionsHandler
{
    fn execute(
        &self,
        query: ListWorkloadIdentityPolicyRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<AcceptedWorkloadIdentityPolicyRevision>>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .list_revisions(ListWorkloadIdentityPolicyRevisionsRead {
                    installation_id,
                    organization_id: query.organization_id,
                    policy_id: query.policy_id,
                    limit: query.limit,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(value) => Ok(Ok(value)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
