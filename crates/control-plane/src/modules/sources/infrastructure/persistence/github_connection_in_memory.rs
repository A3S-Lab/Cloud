use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
use crate::modules::sources::domain::{
    CompleteGithubConnection, GithubConnection, GithubConnectionFlow, GithubConnectionFlowError,
    IGithubConnectionRepository,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryGithubConnectionRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    flows: BTreeMap<OrganizationId, GithubConnectionFlow>,
    connections: BTreeMap<OrganizationId, GithubConnection>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryGithubConnectionRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn flows(&self) -> Vec<GithubConnectionFlow> {
        self.state.read().await.flows.values().cloned().collect()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IGithubConnectionRepository for InMemoryGithubConnectionRepository {
    async fn begin_flow(
        &self,
        flow: GithubConnectionFlow,
    ) -> Result<GithubConnectionFlow, RepositoryError> {
        let mut state = self.state.write().await;
        if state.connections.contains_key(&flow.organization_id) {
            return Err(RepositoryError::Conflict(
                "organization already has a GitHub source connection".into(),
            ));
        }
        if state.flows.iter().any(|(organization_id, existing)| {
            *organization_id != flow.organization_id && existing.state_digest == flow.state_digest
        }) {
            return Err(RepositoryError::Conflict(
                "GitHub connection state collision".into(),
            ));
        }
        state.flows.insert(flow.organization_id, flow.clone());
        Ok(flow)
    }

    async fn prepare_oauth(
        &self,
        installation_state_digest: &str,
        installation_id: crate::modules::sources::domain::GithubInstallationId,
        oauth_state_digest: String,
        pkce_verifier_digest: String,
        now: DateTime<Utc>,
    ) -> Result<GithubConnectionFlow, RepositoryError> {
        let mut state = self.state.write().await;
        let organization_id = state
            .flows
            .iter()
            .find_map(|(organization_id, flow)| {
                (flow.state_digest == installation_state_digest).then_some(*organization_id)
            })
            .ok_or(RepositoryError::NotFound)?;
        if state.flows.iter().any(|(candidate_id, flow)| {
            *candidate_id != organization_id && flow.state_digest == oauth_state_digest
        }) {
            return Err(RepositoryError::Conflict(
                "GitHub connection state collision".into(),
            ));
        }
        let flow = state
            .flows
            .get_mut(&organization_id)
            .ok_or(RepositoryError::NotFound)?;
        flow.prepare_oauth(
            installation_id,
            oauth_state_digest,
            pkce_verifier_digest,
            now,
        )
        .map_err(flow_error)?;
        Ok(flow.clone())
    }

    async fn find_oauth_flow(
        &self,
        oauth_state_digest: &str,
        pkce_verifier_digest: &str,
        now: DateTime<Utc>,
    ) -> Result<GithubConnectionFlow, RepositoryError> {
        let state = self.state.read().await;
        let flow = state
            .flows
            .values()
            .find(|flow| flow.state_digest == oauth_state_digest)
            .ok_or(RepositoryError::NotFound)?;
        flow.require_oauth(oauth_state_digest, pkce_verifier_digest, now)
            .map_err(flow_error)?;
        Ok(flow.clone())
    }

    async fn complete(
        &self,
        request: CompleteGithubConnection,
    ) -> Result<GithubConnection, RepositoryError> {
        let mut state = self.state.write().await;
        let organization_id = state
            .flows
            .iter()
            .find_map(|(organization_id, flow)| {
                (flow.id == request.flow_id).then_some(*organization_id)
            })
            .ok_or(RepositoryError::NotFound)?;
        if organization_id != request.connection.organization_id {
            return Err(RepositoryError::Conflict(
                "GitHub connection flow organization changed".into(),
            ));
        }
        if state.connections.contains_key(&organization_id)
            || state.connections.values().any(|connection| {
                connection.installation_id == request.connection.installation_id
                    || (connection.account_kind == request.connection.account_kind
                        && connection.account_id == request.connection.account_id)
            })
        {
            return Err(RepositoryError::Conflict(
                "GitHub installation or account is already connected".into(),
            ));
        }
        let flow = state
            .flows
            .get_mut(&organization_id)
            .ok_or(RepositoryError::NotFound)?;
        if flow.installation_id != Some(request.connection.installation_id) {
            return Err(RepositoryError::Conflict(
                "GitHub connection flow installation changed".into(),
            ));
        }
        flow.complete(request.completed_at).map_err(flow_error)?;
        state
            .connections
            .insert(organization_id, request.connection.clone());
        state.outbox.push(request.event);
        Ok(request.connection)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<GithubConnection>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .connections
            .get(&organization_id)
            .cloned())
    }
}

fn flow_error(error: GithubConnectionFlowError) -> RepositoryError {
    RepositoryError::Conflict(error.to_string())
}
