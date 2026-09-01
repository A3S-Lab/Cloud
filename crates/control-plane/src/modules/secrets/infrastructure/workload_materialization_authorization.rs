use crate::modules::secrets::application::{
    ISecretMaterializationAuthorizer, SecretMaterializationAuthorization,
    SecretMaterializationAuthorizationError, SecretMaterializationAuthorizationRequest,
};
use crate::modules::workloads::{
    IWorkloadSecretMaterializationAuthorizationQueryPort,
    WorkloadSecretMaterializationAuthorizationQuery,
};
use async_trait::async_trait;
use std::sync::Arc;

/// The sole anti-corruption adapter from Workloads deployment authority to the
/// exact materialization evidence owned by Secrets.
#[derive(Clone)]
pub struct WorkloadsSecretMaterializationAuthorizerAdapter {
    workloads: Arc<dyn IWorkloadSecretMaterializationAuthorizationQueryPort>,
}

impl WorkloadsSecretMaterializationAuthorizerAdapter {
    pub fn new(workloads: Arc<dyn IWorkloadSecretMaterializationAuthorizationQueryPort>) -> Self {
        Self { workloads }
    }
}

#[async_trait]
impl ISecretMaterializationAuthorizer for WorkloadsSecretMaterializationAuthorizerAdapter {
    async fn authorize(
        &self,
        request: SecretMaterializationAuthorizationRequest,
    ) -> Result<SecretMaterializationAuthorization, SecretMaterializationAuthorizationError> {
        request
            .validate()
            .map_err(SecretMaterializationAuthorizationError::Unavailable)?;
        let query = WorkloadSecretMaterializationAuthorizationQuery::new(
            request.organization_id(),
            request.node_id(),
            request.workload_revision_id(),
            request.secret_id(),
            request.secret_version(),
        )
        .map_err(SecretMaterializationAuthorizationError::Unavailable)?;
        let authorization = self
            .workloads
            .find_authorization(query)
            .await
            .map_err(|error| {
                SecretMaterializationAuthorizationError::Unavailable(error.to_string())
            })?
            .ok_or(SecretMaterializationAuthorizationError::Forbidden)?;
        authorization
            .validate()
            .map_err(SecretMaterializationAuthorizationError::Unavailable)?;
        if authorization.organization_id() != request.organization_id()
            || authorization.workload_revision_id() != request.workload_revision_id()
            || authorization.node_id() != request.node_id()
            || authorization.secret_id() != request.secret_id()
            || authorization.secret_version() != request.secret_version()
        {
            return Err(SecretMaterializationAuthorizationError::Unavailable(
                "Workloads returned inconsistent Secret authorization evidence".into(),
            ));
        }
        let authorization = SecretMaterializationAuthorization::new(
            authorization.organization_id(),
            authorization.project_id(),
            authorization.environment_id(),
            authorization.node_id(),
            authorization.workload_revision_id(),
            authorization.secret_id(),
            authorization.secret_version(),
        )
        .map_err(SecretMaterializationAuthorizationError::Unavailable)?;
        authorization
            .validate_for(&request)
            .map_err(SecretMaterializationAuthorizationError::Unavailable)?;
        Ok(authorization)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, SecretId, WorkloadId,
        WorkloadRevisionId,
    };
    use crate::modules::workloads::{
        AuthorizedWorkloadSecretMaterialization, AUTHORIZED_WORKLOAD_SECRET_MATERIALIZATION_SCHEMA,
    };
    use std::sync::Mutex;

    struct FixedWorkloadsAuthorization {
        outcome: Result<Option<AuthorizedWorkloadSecretMaterialization>, RepositoryError>,
        queries: Mutex<Vec<WorkloadSecretMaterializationAuthorizationQuery>>,
    }

    #[async_trait]
    impl IWorkloadSecretMaterializationAuthorizationQueryPort for FixedWorkloadsAuthorization {
        async fn find_authorization(
            &self,
            query: WorkloadSecretMaterializationAuthorizationQuery,
        ) -> Result<Option<AuthorizedWorkloadSecretMaterialization>, RepositoryError> {
            self.queries.lock().expect("query lock").push(query);
            self.outcome.clone()
        }
    }

    struct Fixture {
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_id: WorkloadId,
        revision_id: WorkloadRevisionId,
        node_id: NodeId,
        secret_id: SecretId,
        version: u64,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                workload_id: WorkloadId::new(),
                revision_id: WorkloadRevisionId::new(),
                node_id: NodeId::new(),
                secret_id: SecretId::new(),
                version: 3,
            }
        }

        fn request(&self) -> SecretMaterializationAuthorizationRequest {
            SecretMaterializationAuthorizationRequest::new(
                self.organization_id,
                self.node_id,
                self.revision_id,
                self.secret_id,
                self.version,
            )
            .expect("request")
        }

        fn owner_fact(&self) -> AuthorizedWorkloadSecretMaterialization {
            self.owner_fact_for_node(self.node_id)
        }

        fn owner_fact_for_node(&self, node_id: NodeId) -> AuthorizedWorkloadSecretMaterialization {
            serde_json::from_value(serde_json::json!({
                "schema": AUTHORIZED_WORKLOAD_SECRET_MATERIALIZATION_SCHEMA,
                "organizationId": self.organization_id,
                "projectId": self.project_id,
                "environmentId": self.environment_id,
                "workloadId": self.workload_id,
                "workloadRevisionId": self.revision_id,
                "nodeId": node_id,
                "secretId": self.secret_id,
                "secretVersion": self.version,
            }))
            .expect("owner fact")
        }
    }

    #[tokio::test]
    async fn adapter_projects_one_exact_workloads_fact_into_secrets_evidence() {
        let fixture = Fixture::new();
        let owner = Arc::new(FixedWorkloadsAuthorization {
            outcome: Ok(Some(fixture.owner_fact())),
            queries: Mutex::new(Vec::new()),
        });
        let adapter = WorkloadsSecretMaterializationAuthorizerAdapter::new(owner.clone());

        let evidence = adapter
            .authorize(fixture.request())
            .await
            .expect("authorization");

        assert_eq!(evidence.project_id(), fixture.project_id);
        assert_eq!(evidence.environment_id(), fixture.environment_id);
        assert_eq!(owner.queries.lock().expect("query lock").len(), 1);
    }

    #[tokio::test]
    async fn adapter_preserves_denial_and_hides_owner_failures() {
        let fixture = Fixture::new();
        let denied = WorkloadsSecretMaterializationAuthorizerAdapter::new(Arc::new(
            FixedWorkloadsAuthorization {
                outcome: Ok(None),
                queries: Mutex::new(Vec::new()),
            },
        ));
        let unavailable = WorkloadsSecretMaterializationAuthorizerAdapter::new(Arc::new(
            FixedWorkloadsAuthorization {
                outcome: Err(RepositoryError::Storage("fixture".into())),
                queries: Mutex::new(Vec::new()),
            },
        ));
        let drifted = WorkloadsSecretMaterializationAuthorizerAdapter::new(Arc::new(
            FixedWorkloadsAuthorization {
                outcome: Ok(Some(fixture.owner_fact_for_node(NodeId::new()))),
                queries: Mutex::new(Vec::new()),
            },
        ));

        assert!(matches!(
            denied.authorize(fixture.request()).await,
            Err(SecretMaterializationAuthorizationError::Forbidden)
        ));
        assert!(matches!(
            unavailable.authorize(fixture.request()).await,
            Err(SecretMaterializationAuthorizationError::Unavailable(_))
        ));
        assert!(matches!(
            drifted.authorize(fixture.request()).await,
            Err(SecretMaterializationAuthorizationError::Unavailable(_))
        ));
    }
}
