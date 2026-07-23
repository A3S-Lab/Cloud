use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, SourceConnectionId};
use crate::modules::sources::domain::{
    CompleteGithubConnection, GitProvider, GithubConnection, GithubConnectionFlow,
    GithubConnectionFlowError, GithubConnectionLifecycleAcceptance, GithubConnectionReconciled,
    GithubInstallationId, IGithubConnectionRepository, PersistGithubProviderReconciliation,
    ReconcileGithubConnectionLifecycle,
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

#[derive(Clone, Default)]
struct State {
    flows: BTreeMap<OrganizationId, GithubConnectionFlow>,
    connections: BTreeMap<SourceConnectionId, GithubConnection>,
    lifecycle_inbox: BTreeMap<(String, String), LifecycleReceipt>,
    outbox: Vec<DomainEventEnvelope>,
}

#[derive(Clone, PartialEq, Eq)]
struct LifecycleReceipt {
    event: String,
    action: String,
    subject_kind: String,
    subject_id: u64,
    payload_digest: String,
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

    pub async fn connections(&self) -> Vec<GithubConnection> {
        self.state
            .read()
            .await
            .connections
            .values()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl IGithubConnectionRepository for InMemoryGithubConnectionRepository {
    async fn begin_flow(
        &self,
        flow: GithubConnectionFlow,
    ) -> Result<GithubConnectionFlow, RepositoryError> {
        let mut state = self.state.write().await;
        if state.connections.values().any(|connection| {
            connection.organization_id == flow.organization_id && connection.blocks_reconnection()
        }) {
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
        if !request.connection.is_authoritative()
            || request.connection.aggregate_version != 1
            || request.connection.updated_at != request.connection.connected_at
            || request.connection.provider_checked_at != request.connection.connected_at
            || request.connection.provider_check_attempted_at != request.connection.connected_at
            || request.connection.provider_next_check_at != request.connection.connected_at
            || request.connection.provider_check_failures != 0
            || request.connection.provider_check_error.is_some()
        {
            return Err(RepositoryError::Conflict(
                "new GitHub connection is not active at its initial version".into(),
            ));
        }
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
        if state.connections.contains_key(&request.connection.id) {
            return Err(RepositoryError::Conflict(
                "GitHub source connection ID is already in use".into(),
            ));
        }
        if state.connections.values().any(|connection| {
            connection.blocks_reconnection()
                && (connection.organization_id == organization_id
                    || connection.installation_id == request.connection.installation_id
                    || (connection.account_kind == request.connection.account_kind
                        && connection.account_id == request.connection.account_id))
        }) {
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
            .insert(request.connection.id, request.connection.clone());
        state.outbox.push(request.event);
        Ok(request.connection)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<GithubConnection>, RepositoryError> {
        Ok(current_connection(
            self.state.read().await.connections.values(),
            organization_id,
        ))
    }

    async fn find_authoritative_by_installation(
        &self,
        installation_id: GithubInstallationId,
    ) -> Result<Option<GithubConnection>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .connections
            .values()
            .find(|connection| {
                connection.installation_id == installation_id && connection.is_authoritative()
            })
            .cloned())
    }

    async fn find_provider_check_candidates(
        &self,
        due_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<GithubConnection>, RepositoryError> {
        let mut connections = self
            .state
            .read()
            .await
            .connections
            .values()
            .filter(|connection| {
                connection.needs_provider_check() && connection.provider_next_check_at <= due_at
            })
            .cloned()
            .collect::<Vec<_>>();
        connections.sort_by_key(|connection| {
            (
                connection.provider_next_check_at,
                connection.organization_id,
                connection.id,
            )
        });
        connections.truncate(limit);
        Ok(connections)
    }

    async fn save_provider_reconciliation(
        &self,
        request: PersistGithubProviderReconciliation,
    ) -> Result<GithubConnection, RepositoryError> {
        let mut state = self.state.write().await;
        let current = state
            .connections
            .get(&request.connection.id)
            .ok_or(RepositoryError::NotFound)?;
        if current.aggregate_version != request.expected_version
            || request.connection.aggregate_version
                != request.expected_version.checked_add(1).ok_or_else(|| {
                    RepositoryError::Conflict(
                        "GitHub connection aggregate version overflowed".into(),
                    )
                })?
            || current.organization_id != request.connection.organization_id
            || current.installation_id != request.connection.installation_id
            || current.account_id != request.connection.account_id
            || current.account_kind != request.connection.account_kind
            || current.verified_by_user_id != request.connection.verified_by_user_id
            || current.connected_at != request.connection.connected_at
        {
            return Err(RepositoryError::Conflict(
                "GitHub provider reconciliation lost its connection version".into(),
            ));
        }
        state
            .connections
            .insert(request.connection.id, request.connection.clone());
        if let Some(event) = request.event {
            state.outbox.push(event);
        }
        Ok(request.connection)
    }

    async fn reconcile_lifecycle(
        &self,
        request: ReconcileGithubConnectionLifecycle,
    ) -> Result<GithubConnectionLifecycleAcceptance, RepositoryError> {
        if request.lifecycle.provider != GitProvider::Github
            || !valid_payload_digest(&request.lifecycle.payload_digest)
        {
            return Err(RepositoryError::Conflict(
                "GitHub connection lifecycle delivery is invalid".into(),
            ));
        }
        let mut state = self.state.write().await;
        let key = (
            request.lifecycle.provider.as_str().to_owned(),
            request.lifecycle.delivery_id.as_str().to_owned(),
        );
        let receipt = LifecycleReceipt {
            event: request.lifecycle.change.event_name().into(),
            action: request.lifecycle.change.action_name().into(),
            subject_kind: request.lifecycle.change.subject_kind().into(),
            subject_id: request.lifecycle.change.subject_id(),
            payload_digest: request.lifecycle.payload_digest.clone(),
        };
        if let Some(existing) = state.lifecycle_inbox.get(&key) {
            if existing != &receipt {
                return Err(RepositoryError::Conflict(
                    "GitHub lifecycle delivery ID was reused with another payload".into(),
                ));
            }
            return Ok(GithubConnectionLifecycleAcceptance {
                replayed: true,
                connections: Vec::new(),
            });
        }
        let mut next = state.clone();
        next.lifecycle_inbox.insert(key, receipt);
        let mut reconciled = Vec::new();
        for connection in next.connections.values_mut() {
            if connection
                .reconcile(&request.lifecycle.change, request.received_at)
                .map_err(RepositoryError::Storage)?
            {
                next.outbox.push(
                    GithubConnectionReconciled::envelope(connection, request.correlation_id)
                        .map_err(|error| RepositoryError::Storage(error.to_string()))?,
                );
                reconciled.push(connection.clone());
            }
        }
        reconciled.sort_by_key(|connection| (connection.organization_id, connection.id));
        *state = next;
        Ok(GithubConnectionLifecycleAcceptance {
            replayed: false,
            connections: reconciled,
        })
    }
}

fn current_connection<'a>(
    connections: impl Iterator<Item = &'a GithubConnection>,
    organization_id: OrganizationId,
) -> Option<GithubConnection> {
    connections
        .filter(|connection| connection.organization_id == organization_id)
        .max_by_key(|connection| {
            (
                connection.blocks_reconnection(),
                connection.connected_at,
                connection.id,
            )
        })
        .cloned()
}

fn valid_payload_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn flow_error(error: GithubConnectionFlowError) -> RepositoryError {
    RepositoryError::Conflict(error.to_string())
}
