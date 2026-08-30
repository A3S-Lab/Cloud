use crate::modules::identity::application::{
    IWorkloadRuntimeExecutionAuthorizationQueryPort, WorkloadRuntimeExecutionAuthorizationQuery,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::{
    AdmittedWorkloadRuntimeExecution, DeploymentRuntimeExecutionAdmissionRequest,
    IWorkloadRuntimeExecutionAdmissionPort,
};
use crate::modules::workloads::domain::entities::WorkloadRuntimeExecutionBinding;
use async_trait::async_trait;
use std::sync::Arc;

/// Workloads anti-corruption adapter for the sole Identity owner fact. It
/// creates no policy lifecycle, cache, retry, lock, queue, or credential rule.
pub struct IdentityWorkloadRuntimeExecutionAdmissionAdapter {
    identity: Arc<dyn IWorkloadRuntimeExecutionAuthorizationQueryPort>,
}

impl IdentityWorkloadRuntimeExecutionAdmissionAdapter {
    pub fn new(identity: Arc<dyn IWorkloadRuntimeExecutionAuthorizationQueryPort>) -> Self {
        Self { identity }
    }
}

#[async_trait]
impl IWorkloadRuntimeExecutionAdmissionPort for IdentityWorkloadRuntimeExecutionAdmissionAdapter {
    async fn admit(
        &self,
        request: DeploymentRuntimeExecutionAdmissionRequest,
    ) -> Result<Option<AdmittedWorkloadRuntimeExecution>, RepositoryError> {
        request.validate().map_err(admission_error)?;
        let query = WorkloadRuntimeExecutionAuthorizationQuery::new(
            request.organization_id(),
            request.workload_id(),
        )
        .map_err(admission_error)?;
        let Some(authorization) = self.identity.find_current_authorization(query).await? else {
            return Ok(None);
        };
        authorization.validate().map_err(owner_fact_error)?;
        if authorization.organization_id() != request.organization_id()
            || authorization.project_id() != request.project_id()
            || authorization.environment_id() != request.environment_id()
            || authorization.workload_id() != request.workload_id()
            || authorization.workload_revision_id() != request.workload_revision_id()
            || Some(authorization.node_pool_id()) != request.node_pool_id()
        {
            return Err(admission_error(
                "current Identity policy does not authorize the exact Deployment lineage".into(),
            ));
        }
        let execution = WorkloadRuntimeExecutionBinding::new(
            authorization.runtime_class(),
            authorization.isolation_level(),
            authorization.semantics_profile_digest().clone(),
            authorization.identity_attachment_digest().clone(),
        )
        .map_err(admission_error)?;
        AdmittedWorkloadRuntimeExecution::new(
            authorization.node_pool_id(),
            execution,
            authorization.authorized_at(),
        )
        .map(Some)
        .map_err(admission_error)
    }
}

fn admission_error(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "Deployment Runtime execution admission rejected: {error}"
    ))
}

fn owner_fact_error(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "invalid Identity Runtime execution owner fact: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::published::{
        WorkloadRuntimeExecutionAuthorization, WORKLOAD_RUNTIME_EXECUTION_AUTHORIZATION_SCHEMA,
    };
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, DeploymentId, EnvironmentId, NodePoolId, OrganizationId, ProjectId,
        Sha256Digest, WorkloadId, WorkloadRevisionId,
    };
    use a3s_runtime::contract::{IsolationLevel, RuntimeUnitClass};
    use chrono::Utc;

    struct FixedIdentityOwner {
        authorization: Option<WorkloadRuntimeExecutionAuthorization>,
    }

    #[async_trait]
    impl IWorkloadRuntimeExecutionAuthorizationQueryPort for FixedIdentityOwner {
        async fn find_current_authorization(
            &self,
            query: WorkloadRuntimeExecutionAuthorizationQuery,
        ) -> Result<Option<WorkloadRuntimeExecutionAuthorization>, RepositoryError> {
            query.validate().map_err(RepositoryError::Conflict)?;
            Ok(self.authorization.clone())
        }
    }

    struct Fixture {
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        deployment_id: DeploymentId,
        node_pool_id: NodePoolId,
        semantics: Sha256Digest,
        attachment: Sha256Digest,
        authorized_at: chrono::DateTime<Utc>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                workload_id: WorkloadId::new(),
                workload_revision_id: WorkloadRevisionId::new(),
                deployment_id: DeploymentId::new(),
                node_pool_id: NodePoolId::new(),
                semantics: Sha256Digest::from_bytes(b"semantics"),
                attachment: Sha256Digest::from_bytes(b"identity attachment"),
                authorized_at: canonical_timestamp(Utc::now()),
            }
        }

        fn request(&self) -> DeploymentRuntimeExecutionAdmissionRequest {
            DeploymentRuntimeExecutionAdmissionRequest::new(
                self.organization_id,
                self.project_id,
                self.environment_id,
                self.workload_id,
                self.workload_revision_id,
                self.deployment_id,
                Some(self.node_pool_id),
            )
            .expect("admission request")
        }

        fn authorization(&self) -> WorkloadRuntimeExecutionAuthorization {
            serde_json::from_value(serde_json::json!({
                "schema": WORKLOAD_RUNTIME_EXECUTION_AUTHORIZATION_SCHEMA,
                "organizationId": self.organization_id,
                "projectId": self.project_id,
                "environmentId": self.environment_id,
                "workloadId": self.workload_id,
                "workloadRevisionId": self.workload_revision_id,
                "nodePoolId": self.node_pool_id,
                "runtimeClass": RuntimeUnitClass::Service,
                "isolationLevel": IsolationLevel::Confidential,
                "semanticsProfileDigest": self.semantics,
                "identityAttachmentDigest": self.attachment,
                "authorizedAt": self.authorized_at,
            }))
            .expect("owner authorization")
        }
    }

    #[tokio::test]
    async fn maps_only_an_exact_owner_fact_to_generic_runtime_semantics() {
        let fixture = Fixture::new();
        let adapter =
            IdentityWorkloadRuntimeExecutionAdmissionAdapter::new(Arc::new(FixedIdentityOwner {
                authorization: Some(fixture.authorization()),
            }));

        let admitted = adapter
            .admit(fixture.request())
            .await
            .expect("admission")
            .expect("admitted semantics");

        assert_eq!(admitted.node_pool_id(), fixture.node_pool_id);
        assert_eq!(admitted.authorized_at(), fixture.authorized_at);
        assert_eq!(
            admitted.execution().runtime_class(),
            RuntimeUnitClass::Service
        );
        assert_eq!(
            admitted.execution().isolation(),
            IsolationLevel::Confidential
        );
        assert_eq!(
            admitted.execution().semantics_profile_digest(),
            &fixture.semantics
        );
        assert_eq!(
            admitted.execution().identity_attachment_digest(),
            &fixture.attachment
        );
    }

    #[tokio::test]
    async fn rejects_owner_lineage_drift_and_preserves_an_absent_policy() {
        let fixture = Fixture::new();
        let drifted_request = DeploymentRuntimeExecutionAdmissionRequest::new(
            fixture.organization_id,
            ProjectId::new(),
            fixture.environment_id,
            fixture.workload_id,
            fixture.workload_revision_id,
            fixture.deployment_id,
            Some(fixture.node_pool_id),
        )
        .expect("drifted request");
        let adapter =
            IdentityWorkloadRuntimeExecutionAdmissionAdapter::new(Arc::new(FixedIdentityOwner {
                authorization: Some(fixture.authorization()),
            }));

        let error = adapter
            .admit(drifted_request)
            .await
            .expect_err("lineage drift must fail closed");
        assert!(matches!(error, RepositoryError::Conflict(_)));

        let no_policy =
            IdentityWorkloadRuntimeExecutionAdmissionAdapter::new(Arc::new(FixedIdentityOwner {
                authorization: None,
            }));
        assert!(no_policy
            .admit(fixture.request())
            .await
            .expect("absent policy")
            .is_none());
    }
}
