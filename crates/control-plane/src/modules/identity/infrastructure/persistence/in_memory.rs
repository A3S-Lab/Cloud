use crate::modules::identity::domain::entities::{ApiToken, IdentityBootstrap, Organization};
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IOrganizationRepository,
};
use crate::modules::identity::domain::value_objects::ApiTokenDigest;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryIdentityRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    organizations: BTreeMap<OrganizationId, Organization>,
    names: BTreeMap<String, OrganizationId>,
    tokens: BTreeMap<ApiTokenId, ApiToken>,
    token_names: BTreeMap<(OrganizationId, String), ApiTokenId>,
    token_digests: BTreeMap<String, ApiTokenId>,
    idempotency: BTreeMap<(String, String), (String, Value)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryIdentityRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

fn replay<T: DeserializeOwned>(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotentWrite<T>>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((digest, response)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    serde_json::from_value(response.clone())
        .map(|value| {
            Some(IdempotentWrite {
                value,
                replayed: true,
            })
        })
        .map_err(|error| RepositoryError::Storage(error.to_string()))
}

fn remember<T: Serialize>(
    state: &mut State,
    idempotency: IdempotencyRequest,
    response: &T,
) -> Result<(), RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let response = serde_json::to_value(response)
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    state
        .idempotency
        .insert(key, (idempotency.request_digest, response));
    Ok(())
}

#[async_trait]
impl IOrganizationRepository for InMemoryIdentityRepository {
    async fn create(
        &self,
        organization: Organization,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Organization>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(existing) = replay(&state, &idempotency)? {
            return Ok(existing);
        }
        if state.names.contains_key(organization.name.key()) {
            return Err(RepositoryError::Conflict(
                "organization name is already in use".into(),
            ));
        }
        state
            .names
            .insert(organization.name.key().to_owned(), organization.id);
        state
            .organizations
            .insert(organization.id, organization.clone());
        remember(&mut state, idempotency, &organization)?;
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: organization,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<Organization>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .organizations
            .get(&organization_id)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Organization>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .organizations
            .values()
            .cloned()
            .collect())
    }
}

#[async_trait]
impl IApiTokenRepository for InMemoryIdentityRepository {
    async fn bootstrap(
        &self,
        bootstrap: IdentityBootstrap,
        digest: ApiTokenDigest,
        events: [DomainEventEnvelope; 2],
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<IdentityBootstrap>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(existing) = replay(&state, &idempotency)? {
            return Ok(existing);
        }
        if !state.organizations.is_empty() {
            return Err(RepositoryError::Conflict(
                "Cloud identity has already been bootstrapped".into(),
            ));
        }
        let organization = bootstrap.organization.clone();
        let token = bootstrap.api_token.clone();
        state
            .names
            .insert(organization.name.key().to_owned(), organization.id);
        state.organizations.insert(organization.id, organization);
        state.token_names.insert(
            (token.organization_id, token.name.key().to_owned()),
            token.id,
        );
        state
            .token_digests
            .insert(digest.as_str().to_owned(), token.id);
        state.tokens.insert(token.id, token);
        remember(&mut state, idempotency, &bootstrap)?;
        state.outbox.extend(events);
        Ok(IdempotentWrite {
            value: bootstrap,
            replayed: false,
        })
    }

    async fn create(
        &self,
        token: ApiToken,
        digest: ApiTokenDigest,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(existing) = replay(&state, &idempotency)? {
            return Ok(existing);
        }
        if !state.organizations.contains_key(&token.organization_id) {
            return Err(RepositoryError::NotFound);
        }
        let name_key = (token.organization_id, token.name.key().to_owned());
        if state.token_names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "API token name is already in use".into(),
            ));
        }
        if state.token_digests.contains_key(digest.as_str()) {
            return Err(RepositoryError::Conflict(
                "API token credential is already in use".into(),
            ));
        }
        state.token_names.insert(name_key, token.id);
        state
            .token_digests
            .insert(digest.as_str().to_owned(), token.id);
        state.tokens.insert(token.id, token.clone());
        remember(&mut state, idempotency, &token)?;
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: token,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        token_id: ApiTokenId,
    ) -> Result<Option<ApiToken>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .tokens
            .get(&token_id)
            .filter(|token| token.organization_id == organization_id)
            .cloned())
    }

    async fn authenticate(
        &self,
        digest: &ApiTokenDigest,
        now: DateTime<Utc>,
    ) -> Result<Option<ApiToken>, RepositoryError> {
        let state = self.state.read().await;
        let Some(token_id) = state.token_digests.get(digest.as_str()) else {
            return Ok(None);
        };
        Ok(state
            .tokens
            .get(token_id)
            .filter(|token| token.is_active_at(now))
            .cloned())
    }

    async fn revoke(
        &self,
        token: ApiToken,
        event: Option<DomainEventEnvelope>,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(existing) = replay(&state, &idempotency)? {
            return Ok(existing);
        }
        let existing = state
            .tokens
            .get(&token.id)
            .filter(|stored| stored.organization_id == token.organization_id)
            .ok_or(RepositoryError::NotFound)?;
        if existing.aggregate_version + u64::from(event.is_some()) != token.aggregate_version {
            return Err(RepositoryError::Conflict(
                "API token changed while it was being revoked".into(),
            ));
        }
        state.tokens.insert(token.id, token.clone());
        remember(&mut state, idempotency, &token)?;
        if let Some(event) = event {
            state.outbox.push(event);
        }
        Ok(IdempotentWrite {
            value: token,
            replayed: false,
        })
    }
}
