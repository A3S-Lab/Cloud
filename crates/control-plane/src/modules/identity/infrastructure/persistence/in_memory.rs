use crate::modules::identity::domain::entities::{
    ApiToken, AuthenticatedApiToken, ExternalIdentityLink, IdentityBootstrap, IdentityPrincipal,
    Membership, MembershipInvitation, OidcFlow, Organization, PlatformRbacBootstrap,
    RecipientContact, RecipientContactVerification, RecipientContactVerificationDeliveryRecord,
    ResourceGrant,
};
use crate::modules::identity::domain::events::{
    PlatformRoleBindingChanged, PlatformRolePolicyAccepted,
};
use crate::modules::identity::domain::repositories::{
    BootstrapIdentityWrite, CreateApiTokenWrite, CreateOrganizationWrite, IApiTokenRepository,
    IIdentityBootstrapRepository, IOrganizationRepository, ReadOrganizationCatalog,
};
use crate::modules::identity::domain::services::{
    MembershipAdministration, ResourceAuthorizationDecision,
};
use crate::modules::identity::domain::value_objects::{ApiTokenDigest, ApiTokenScope};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, ExternalIdentityLinkId, IdempotencyRequest, IdempotentWrite, InstallationId,
    MembershipId, MembershipInvitationId, OidcFlowId, OrganizationId, PrincipalId,
    RecipientContactId, RecipientContactVerificationId, RepositoryError, ResourceGrantId,
    Sha256Digest,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub struct InMemoryIdentityRepository {
    installation_id: InstallationId,
    pub(super) state: RwLock<State>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) organizations: BTreeMap<OrganizationId, Organization>,
    pub(super) names: BTreeMap<String, OrganizationId>,
    pub(super) principals: BTreeMap<PrincipalId, IdentityPrincipal>,
    pub(super) memberships: BTreeMap<MembershipId, Membership>,
    pub(super) membership_invitations: BTreeMap<MembershipInvitationId, MembershipInvitation>,
    pub(super) resource_grants: BTreeMap<ResourceGrantId, ResourceGrant>,
    pub(super) resource_authorization_decisions: BTreeMap<Uuid, ResourceAuthorizationDecision>,
    pub(super) membership_subjects: BTreeMap<(OrganizationId, PrincipalId), MembershipId>,
    pub(super) oidc_flows: BTreeMap<OidcFlowId, OidcFlow>,
    pub(super) oidc_flow_states: BTreeMap<Sha256Digest, OidcFlowId>,
    pub(super) external_identity_links: BTreeMap<ExternalIdentityLinkId, ExternalIdentityLink>,
    pub(super) recipient_contacts: BTreeMap<RecipientContactId, RecipientContact>,
    pub(super) recipient_contact_addresses: BTreeMap<(PrincipalId, String), RecipientContactId>,
    pub(super) recipient_contact_verifications:
        BTreeMap<RecipientContactVerificationId, RecipientContactVerification>,
    pub(super) recipient_contact_verification_organizations:
        BTreeMap<RecipientContactVerificationId, OrganizationId>,
    pub(super) recipient_contact_verification_deliveries:
        BTreeMap<RecipientContactVerificationId, RecipientContactVerificationDeliveryRecord>,
    pub(super) tokens: BTreeMap<ApiTokenId, ApiToken>,
    pub(super) token_names: BTreeMap<(OrganizationId, String), ApiTokenId>,
    pub(super) token_digests: BTreeMap<String, ApiTokenId>,
    pub(super) platform_rbac: Option<PlatformRbacBootstrap>,
    pub(super) idempotency: BTreeMap<(String, String), (String, Value)>,
    pub(super) outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryIdentityRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

pub(super) fn replay<T: DeserializeOwned>(
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

pub(super) fn remember<T: Serialize>(
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
        write: CreateOrganizationWrite,
    ) -> Result<IdempotentWrite<Organization>, RepositoryError> {
        let mut state = self.state.write().await;
        let CreateOrganizationWrite {
            organization,
            owner_membership,
            events,
            actor_principal_id,
            idempotency,
            ..
        } = write;
        if let Some(existing) = replay(&state, &idempotency)? {
            return Ok(existing);
        }
        if !state
            .principals
            .get(&actor_principal_id)
            .is_some_and(IdentityPrincipal::is_active)
        {
            return Err(RepositoryError::Forbidden(
                "organization creator is not an active identity principal".into(),
            ));
        }
        if owner_membership.organization_id != organization.id
            || owner_membership.principal_id != actor_principal_id
        {
            return Err(RepositoryError::Storage(
                "organization owner membership does not bind its creator".into(),
            ));
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
        state.membership_subjects.insert(
            (
                owner_membership.organization_id,
                owner_membership.principal_id,
            ),
            owner_membership.id,
        );
        state
            .memberships
            .insert(owner_membership.id, owner_membership);
        remember(&mut state, idempotency, &organization)?;
        state.outbox.extend(events);
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

    async fn list_visible(
        &self,
        read: ReadOrganizationCatalog,
    ) -> Result<Vec<Organization>, RepositoryError> {
        if read.installation_id != self.installation_id {
            return Err(RepositoryError::Forbidden(
                "organization catalog crossed the Installation boundary".into(),
            ));
        }
        let state = self.state.read().await;
        let principal = state
            .principals
            .get(&read.actor_principal_id)
            .filter(|principal| principal.is_active())
            .ok_or_else(|| {
                RepositoryError::Forbidden("organization catalog principal is not active".into())
            })?;
        let credential = state
            .tokens
            .get(&read.credential_id)
            .filter(|credential| {
                credential.principal_id == principal.id
                    && credential.is_active_at(Utc::now())
                    && credential.grants_scope(ApiTokenScope::CLOUD_READ)
            })
            .ok_or_else(|| {
                RepositoryError::Forbidden(
                    "organization catalog credential is not active or lacks cloud:read".into(),
                )
            })?;
        let organization = state
            .organizations
            .get(&credential.organization_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "organization catalog credential has no tenant authority".into(),
                )
            })?;

        // The in-memory adapter has no transactional privileged decision
        // authority. It deliberately exposes only the exact credential's
        // tenant instead of emulating Installation-wide access.
        Ok(vec![organization])
    }
}

#[async_trait]
impl IIdentityBootstrapRepository for InMemoryIdentityRepository {
    async fn installation_id(&self) -> Result<InstallationId, RepositoryError> {
        Ok(self.installation_id)
    }

    async fn bootstrap_identity(
        &self,
        write: BootstrapIdentityWrite,
    ) -> Result<IdempotentWrite<IdentityBootstrap>, RepositoryError> {
        let BootstrapIdentityWrite {
            bootstrap,
            token_digest,
            identity_events,
            request_id,
            idempotency,
        } = write;
        bootstrap.validate().map_err(RepositoryError::Storage)?;
        if bootstrap.platform_rbac.policy.installation_id != self.installation_id {
            return Err(RepositoryError::Storage(
                "identity bootstrap crossed the in-memory Installation boundary".into(),
            ));
        }
        let platform_events = [
            PlatformRolePolicyAccepted::envelope(&bootstrap.platform_rbac.policy, request_id)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?,
            PlatformRoleBindingChanged::created(&bootstrap.platform_rbac.owner_binding, request_id)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?,
        ];
        let mut state = self.state.write().await;
        if let Some(existing) = replay(&state, &idempotency)? {
            return Ok(existing);
        }
        if !state.organizations.is_empty() || state.platform_rbac.is_some() {
            return Err(RepositoryError::Conflict(
                "Cloud identity has already been bootstrapped".into(),
            ));
        }
        let organization = bootstrap.organization.clone();
        let principal = bootstrap.principal.clone();
        let membership = bootstrap.membership.clone();
        let token = bootstrap.api_token.clone();
        state
            .names
            .insert(organization.name.key().to_owned(), organization.id);
        state.organizations.insert(organization.id, organization);
        state.principals.insert(principal.id, principal);
        state.membership_subjects.insert(
            (membership.organization_id, membership.principal_id),
            membership.id,
        );
        state.memberships.insert(membership.id, membership);
        state.token_names.insert(
            (token.organization_id, token.name.key().to_owned()),
            token.id,
        );
        state
            .token_digests
            .insert(token_digest.as_str().to_owned(), token.id);
        state.tokens.insert(token.id, token);
        state.platform_rbac = Some(bootstrap.platform_rbac.clone());
        remember(&mut state, idempotency, &bootstrap)?;
        state.outbox.extend(identity_events);
        state.outbox.extend(platform_events);
        Ok(IdempotentWrite {
            value: bootstrap,
            replayed: false,
        })
    }
}

#[async_trait]
impl IApiTokenRepository for InMemoryIdentityRepository {
    async fn create(
        &self,
        write: CreateApiTokenWrite,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError> {
        let CreateApiTokenWrite {
            token,
            digest,
            event,
            issuer_principal_id,
            idempotency,
        } = write;
        let mut state = self.state.write().await;
        let target_membership = state
            .membership_subjects
            .get(&(token.organization_id, token.principal_id))
            .and_then(|id| state.memberships.get(id))
            .filter(|membership| membership.is_active())
            .cloned();
        let Some(target_membership) = target_membership else {
            return Err(RepositoryError::Forbidden(
                "API token principal is not an active organization member".into(),
            ));
        };
        let target_principal_is_active = state
            .principals
            .get(&token.principal_id)
            .is_some_and(IdentityPrincipal::is_active);
        if !target_principal_is_active {
            return Err(RepositoryError::Forbidden(
                "API token principal is not an active organization member".into(),
            ));
        }
        if token.principal_id != issuer_principal_id {
            let issuer = state
                .membership_subjects
                .get(&(token.organization_id, issuer_principal_id))
                .and_then(|id| state.memberships.get(id))
                .cloned();
            MembershipAdministration::authorize(
                issuer.as_ref(),
                token.organization_id,
                target_membership.role,
                None,
            )
            .map_err(RepositoryError::Forbidden)?;
        }
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

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<ApiToken>, RepositoryError> {
        let mut tokens = self
            .state
            .read()
            .await
            .tokens
            .values()
            .filter(|token| token.organization_id == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        tokens.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        Ok(tokens)
    }

    async fn authenticate(
        &self,
        digest: &ApiTokenDigest,
        now: DateTime<Utc>,
    ) -> Result<Option<AuthenticatedApiToken>, RepositoryError> {
        let state = self.state.read().await;
        let Some(token_id) = state.token_digests.get(digest.as_str()) else {
            return Ok(None);
        };
        let Some(api_token) = state
            .tokens
            .get(token_id)
            .filter(|token| token.is_active_at(now))
            .cloned()
        else {
            return Ok(None);
        };
        let Some(principal) = state
            .principals
            .get(&api_token.principal_id)
            .filter(|principal| principal.is_active())
            .cloned()
        else {
            return Ok(None);
        };
        let membership = state
            .membership_subjects
            .get(&(api_token.organization_id, api_token.principal_id))
            .and_then(|id| state.memberships.get(id))
            .filter(|membership| membership.is_active())
            .cloned();
        let is_platform_token = api_token
            .scopes
            .iter()
            .any(|scope| scope.as_str() == ApiTokenScope::PLATFORM_WRITE);
        if membership.is_none() && !is_platform_token {
            return Ok(None);
        }
        Ok(Some(AuthenticatedApiToken {
            api_token,
            principal,
            membership,
        }))
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
