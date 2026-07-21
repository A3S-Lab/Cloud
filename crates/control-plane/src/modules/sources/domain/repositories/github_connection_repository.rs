use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
use crate::modules::sources::domain::entities::{GithubConnection, GithubConnectionFlow};
use crate::modules::sources::domain::{GithubInstallationId, VerifiedGithubConnectionLifecycle};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct CompleteGithubConnection {
    pub flow_id: Uuid,
    pub connection: GithubConnection,
    pub event: DomainEventEnvelope,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ReconcileGithubConnectionLifecycle {
    pub lifecycle: VerifiedGithubConnectionLifecycle,
    pub correlation_id: Uuid,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct GithubConnectionLifecycleAcceptance {
    pub replayed: bool,
    pub connections: Vec<GithubConnection>,
}

#[async_trait]
pub trait IGithubConnectionRepository: Send + Sync {
    async fn begin_flow(
        &self,
        flow: GithubConnectionFlow,
    ) -> Result<GithubConnectionFlow, RepositoryError>;

    async fn prepare_oauth(
        &self,
        installation_state_digest: &str,
        installation_id: crate::modules::sources::domain::GithubInstallationId,
        oauth_state_digest: String,
        pkce_verifier_digest: String,
        now: DateTime<Utc>,
    ) -> Result<GithubConnectionFlow, RepositoryError>;

    async fn find_oauth_flow(
        &self,
        oauth_state_digest: &str,
        pkce_verifier_digest: &str,
        now: DateTime<Utc>,
    ) -> Result<GithubConnectionFlow, RepositoryError>;

    async fn complete(
        &self,
        request: CompleteGithubConnection,
    ) -> Result<GithubConnection, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<GithubConnection>, RepositoryError>;

    async fn find_authoritative_by_installation(
        &self,
        installation_id: GithubInstallationId,
    ) -> Result<Option<GithubConnection>, RepositoryError>;

    async fn reconcile_lifecycle(
        &self,
        request: ReconcileGithubConnectionLifecycle,
    ) -> Result<GithubConnectionLifecycleAcceptance, RepositoryError>;
}
