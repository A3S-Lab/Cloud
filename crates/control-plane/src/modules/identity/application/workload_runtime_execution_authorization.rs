use crate::modules::identity::domain::repositories::{
    IWorkloadIdentityPolicyRepository, ReadCurrentWorkloadIdentityPolicyForRuntime,
};
use crate::modules::identity::published::{
    ValidatedWorkloadRuntimeExecutionAuthorizationProjection, WorkloadRuntimeExecutionAuthorization,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, WorkloadId};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadRuntimeExecutionAuthorizationQuery {
    organization_id: OrganizationId,
    workload_id: WorkloadId,
}

impl WorkloadRuntimeExecutionAuthorizationQuery {
    pub fn new(organization_id: OrganizationId, workload_id: WorkloadId) -> Result<Self, String> {
        let value = Self {
            organization_id,
            workload_id,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.workload_id.as_uuid().is_nil() {
            return Err("workload Runtime authorization query identity is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }
}

#[async_trait]
pub trait IWorkloadRuntimeExecutionAuthorizationQueryPort: Send + Sync {
    async fn find_current_authorization(
        &self,
        query: WorkloadRuntimeExecutionAuthorizationQuery,
    ) -> Result<Option<WorkloadRuntimeExecutionAuthorization>, RepositoryError>;
}

/// Identity owner-side query service. It is the sole interpreter of the
/// current policy head and publishes no policy aggregate or credential rule.
pub struct WorkloadRuntimeExecutionAuthorizationQueryService {
    policies: Arc<dyn IWorkloadIdentityPolicyRepository>,
}

impl WorkloadRuntimeExecutionAuthorizationQueryService {
    pub fn new(policies: Arc<dyn IWorkloadIdentityPolicyRepository>) -> Self {
        Self { policies }
    }
}

#[async_trait]
impl IWorkloadRuntimeExecutionAuthorizationQueryPort
    for WorkloadRuntimeExecutionAuthorizationQueryService
{
    async fn find_current_authorization(
        &self,
        query: WorkloadRuntimeExecutionAuthorizationQuery,
    ) -> Result<Option<WorkloadRuntimeExecutionAuthorization>, RepositoryError> {
        query.validate().map_err(RepositoryError::Conflict)?;
        let policy = self
            .policies
            .read_current_for_runtime(ReadCurrentWorkloadIdentityPolicyForRuntime {
                organization_id: query.organization_id,
                workload_id: query.workload_id,
            })
            .await?;
        let Some(policy) = policy else {
            return Ok(None);
        };
        policy.validate().map_err(owner_projection_error)?;
        let spec = policy.contract.spec();
        if spec.organization_id != query.organization_id || spec.workload_id != query.workload_id {
            return Err(owner_projection_error(
                "Identity repository substituted the requested Workload".into(),
            ));
        }
        WorkloadRuntimeExecutionAuthorization::from_validated_policy(
            ValidatedWorkloadRuntimeExecutionAuthorizationProjection {
                organization_id: spec.organization_id,
                project_id: spec.project_id,
                environment_id: spec.environment_id,
                workload_id: spec.workload_id,
                workload_revision_id: spec.workload_revision_id,
                node_pool_id: spec.node_pool_id,
                runtime_class: spec.runtime_class,
                isolation_level: spec.isolation_level,
                semantics_profile_digest: spec.semantics_profile_digest.clone(),
                identity_attachment_digest: policy.contract.digest().clone(),
                authorized_at: policy.accepted_at,
            },
        )
        .map(Some)
        .map_err(owner_projection_error)
    }
}

fn owner_projection_error(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "invalid Identity workload Runtime authorization projection: {error}"
    ))
}
