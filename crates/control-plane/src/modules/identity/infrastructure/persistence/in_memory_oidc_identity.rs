use super::in_memory::InMemoryIdentityRepository;
use crate::modules::identity::domain::entities::{
    ApiToken, ExternalIdentityLink, OidcFlow, OidcFlowPurpose,
};
use crate::modules::identity::domain::events::{ApiTokenCreated, ExternalIdentityChanged};
use crate::modules::identity::domain::repositories::{
    CompleteOidcLinkWrite, CompleteOidcLoginWrite, IOidcIdentityRepository,
};
use crate::modules::shared_kernel::domain::{
    ExternalIdentityLinkId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

fn flow_error(error: impl ToString) -> RepositoryError {
    RepositoryError::Conflict(error.to_string())
}

#[async_trait]
impl IOidcIdentityRepository for InMemoryIdentityRepository {
    async fn begin_oidc_flow(&self, flow: OidcFlow) -> Result<OidcFlow, RepositoryError> {
        let mut state = self.state.write().await;
        if !state.organizations.contains_key(&flow.organization_id) {
            return Err(RepositoryError::NotFound);
        }
        if let Some(principal_id) = flow.principal_id {
            let active_membership = state
                .membership_subjects
                .get(&(flow.organization_id, principal_id))
                .and_then(|id| state.memberships.get(id))
                .is_some_and(|membership| membership.is_active());
            let active_human = state
                .principals
                .get(&principal_id)
                .is_some_and(|principal| {
                    principal.is_active()
                        && principal.kind
                            == crate::modules::identity::domain::entities::IdentityPrincipalKind::Human
                });
            if !active_membership || !active_human {
                return Err(RepositoryError::Forbidden(
                    "OIDC link flow requires an active human organization member".into(),
                ));
            }
        }
        if state.oidc_flows.contains_key(&flow.id)
            || state.oidc_flow_states.contains_key(&flow.state_digest)
            || state.oidc_flows.values().any(|candidate| {
                candidate.nonce_digest == flow.nonce_digest
                    || candidate.pkce_verifier_digest == flow.pkce_verifier_digest
            })
        {
            return Err(RepositoryError::Conflict(
                "OIDC flow identity is already in use".into(),
            ));
        }
        state
            .oidc_flow_states
            .insert(flow.state_digest.clone(), flow.id);
        state.oidc_flows.insert(flow.id, flow.clone());
        Ok(flow)
    }

    async fn find_pending_oidc_flow(
        &self,
        state_digest: &Sha256Digest,
        now: DateTime<Utc>,
    ) -> Result<Option<OidcFlow>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .oidc_flow_states
            .get(state_digest)
            .and_then(|id| state.oidc_flows.get(id))
            .filter(|flow| flow.consumed_at.is_none() && now < flow.expires_at)
            .cloned())
    }

    async fn complete_oidc_link(
        &self,
        write: CompleteOidcLinkWrite,
    ) -> Result<ExternalIdentityLink, RepositoryError> {
        let mut state = self.state.write().await;
        let mut flow = state
            .oidc_flows
            .get(&write.flow_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if flow.purpose != OidcFlowPurpose::Link {
            return Err(RepositoryError::Conflict(
                "OIDC flow purpose does not permit identity linking".into(),
            ));
        }
        if flow.provider_config_digest != write.provider_config_digest {
            return Err(RepositoryError::Conflict(
                "OIDC provider configuration changed during the flow".into(),
            ));
        }
        flow.consume(
            &write.state_digest,
            &write.nonce_digest,
            &write.pkce_verifier_digest,
            write.completed_at,
        )
        .map_err(flow_error)?;
        let principal_id = flow.principal_id.ok_or_else(|| {
            RepositoryError::Storage("OIDC link flow lost its principal binding".into())
        })?;
        let active_membership = state
            .membership_subjects
            .get(&(flow.organization_id, principal_id))
            .and_then(|id| state.memberships.get(id))
            .is_some_and(|membership| membership.is_active());
        let principal = state
            .principals
            .get(&principal_id)
            .filter(|principal| principal.is_active())
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Forbidden(
                    "OIDC link principal is not an active organization member".into(),
                )
            })?;
        if !active_membership {
            return Err(RepositoryError::Forbidden(
                "OIDC link principal is not an active organization member".into(),
            ));
        }
        let matching_link = state
            .external_identity_links
            .values()
            .find(|link| link.issuer == flow.issuer && link.subject == write.subject)
            .cloned();
        let (link, event_kind) = if let Some(mut link) = matching_link {
            if !link.is_active()
                || link.principal_id != principal_id
                || link.provider_key != flow.provider_key
            {
                return Err(RepositoryError::Conflict(
                    "external identity is already bound and cannot be reassigned".into(),
                ));
            }
            let changed = link
                .record_verification(write.completed_at)
                .map_err(RepositoryError::Conflict)?;
            (link, changed.then_some(false))
        } else {
            if state.external_identity_links.values().any(|link| {
                link.is_active() && link.principal_id == principal_id && link.issuer == flow.issuer
            }) {
                return Err(RepositoryError::Conflict(
                    "principal already has an active external identity for this issuer".into(),
                ));
            }
            (
                ExternalIdentityLink::create(
                    ExternalIdentityLinkId::new(),
                    flow.provider_key.clone(),
                    flow.issuer.clone(),
                    write.subject,
                    &principal,
                    write.completed_at,
                )
                .map_err(RepositoryError::Conflict)?,
                Some(true),
            )
        };
        let organization_id = flow.organization_id;
        state.oidc_flows.insert(flow.id, flow);
        state.external_identity_links.insert(link.id, link.clone());
        if let Some(newly_linked) = event_kind {
            let event = if newly_linked {
                ExternalIdentityChanged::linked(&link, organization_id, write.request_id)
            } else {
                ExternalIdentityChanged::verified(&link, organization_id, write.request_id)
            }
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
            state.outbox.push(event);
        }
        Ok(link)
    }

    async fn complete_oidc_login(
        &self,
        write: CompleteOidcLoginWrite,
    ) -> Result<ApiToken, RepositoryError> {
        let mut state = self.state.write().await;
        let mut flow = state
            .oidc_flows
            .get(&write.flow_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if flow.purpose != OidcFlowPurpose::Login {
            return Err(RepositoryError::Conflict(
                "OIDC flow purpose does not permit login".into(),
            ));
        }
        if flow.provider_config_digest != write.provider_config_digest {
            return Err(RepositoryError::Conflict(
                "OIDC provider configuration changed during the flow".into(),
            ));
        }
        flow.consume(
            &write.state_digest,
            &write.nonce_digest,
            &write.pkce_verifier_digest,
            write.completed_at,
        )
        .map_err(flow_error)?;
        let link = state
            .external_identity_links
            .values()
            .find(|link| {
                link.is_active()
                    && link.provider_key == flow.provider_key
                    && link.issuer == flow.issuer
                    && link.subject == write.subject
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let principal = state
            .principals
            .get(&link.principal_id)
            .filter(|principal| principal.is_active())
            .ok_or(RepositoryError::NotFound)?;
        if principal.kind
            != crate::modules::identity::domain::entities::IdentityPrincipalKind::Human
        {
            return Err(RepositoryError::NotFound);
        }
        let active_membership = state
            .membership_subjects
            .get(&(flow.organization_id, link.principal_id))
            .and_then(|id| state.memberships.get(id))
            .is_some_and(|membership| membership.is_active());
        if !active_membership {
            return Err(RepositoryError::NotFound);
        }
        let token = ApiToken::issue_oidc_login(
            write.token_id,
            flow.organization_id,
            link.principal_id,
            write.token_name,
            write.completed_at,
            write.token_expires_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let name_key = (token.organization_id, token.name.key().to_owned());
        if state.tokens.contains_key(&write.token_id)
            || state.token_names.contains_key(&name_key)
            || state
                .token_digests
                .contains_key(write.token_digest.as_str())
        {
            return Err(RepositoryError::Conflict(
                "OIDC login credential identity is already in use".into(),
            ));
        }
        let event = ApiTokenCreated::envelope(&token, write.request_id)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        state.oidc_flows.insert(flow.id, flow);
        state.token_names.insert(name_key, token.id);
        state
            .token_digests
            .insert(write.token_digest.as_str().to_owned(), token.id);
        state.tokens.insert(token.id, token.clone());
        state.outbox.push(event);
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{
        IdentityPrincipal, IdentityPrincipalKind, Membership, Organization,
    };
    use crate::modules::identity::domain::value_objects::{
        ApiTokenDigest, ApiTokenName, ExternalIdentitySubject, MembershipRole, OidcIssuer,
        OidcProviderKey, OrganizationName,
    };
    use crate::modules::shared_kernel::domain::{
        ApiTokenId, MembershipId, OidcFlowId, OrganizationId, PrincipalId, ResourceName,
    };
    use chrono::Duration;
    use uuid::Uuid;

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    async fn identity() -> (
        InMemoryIdentityRepository,
        OrganizationId,
        IdentityPrincipal,
    ) {
        let repository = InMemoryIdentityRepository::new();
        let now = Utc::now();
        let organization = Organization::create(
            OrganizationId::new(),
            OrganizationName::parse("OIDC tenant").expect("organization"),
            now,
        );
        let principal = IdentityPrincipal::create(
            PrincipalId::new(),
            IdentityPrincipalKind::Human,
            ResourceName::parse("OIDC human").expect("principal"),
            now,
        );
        let membership = Membership::create(
            MembershipId::new(),
            organization.id,
            principal.id,
            MembershipRole::Owner,
            now,
        );
        {
            let mut state = repository.state.write().await;
            state.principals.insert(principal.id, principal.clone());
            state
                .names
                .insert(organization.name.key().to_owned(), organization.id);
            state
                .organizations
                .insert(organization.id, organization.clone());
            state
                .membership_subjects
                .insert((organization.id, principal.id), membership.id);
            state.memberships.insert(membership.id, membership);
        }
        (repository, organization.id, principal)
    }

    fn flow(
        organization_id: OrganizationId,
        purpose: OidcFlowPurpose,
        principal_id: Option<PrincipalId>,
        byte: char,
        now: DateTime<Utc>,
    ) -> OidcFlow {
        OidcFlow::begin(
            OidcFlowId::new(),
            organization_id,
            OidcProviderKey::parse("workforce").expect("provider"),
            OidcIssuer::parse("https://identity.example.test/tenant").expect("issuer"),
            digest('d'),
            purpose,
            principal_id,
            digest(byte),
            digest(char::from_u32(byte as u32 + 1).expect("nonce byte")),
            digest(char::from_u32(byte as u32 + 2).expect("PKCE byte")),
            now,
            now + Duration::minutes(5),
        )
        .expect("flow")
    }

    async fn link_identity(
        repository: &InMemoryIdentityRepository,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> ExternalIdentityLink {
        let flow = flow(
            organization_id,
            OidcFlowPurpose::Link,
            Some(principal_id),
            'a',
            now,
        );
        repository
            .begin_oidc_flow(flow.clone())
            .await
            .expect("begin link flow");
        repository
            .complete_oidc_link(CompleteOidcLinkWrite {
                flow_id: flow.id,
                provider_config_digest: flow.provider_config_digest,
                state_digest: flow.state_digest,
                nonce_digest: flow.nonce_digest,
                pkce_verifier_digest: flow.pkce_verifier_digest,
                subject: ExternalIdentitySubject::parse("subject-42").expect("subject"),
                completed_at: now + Duration::seconds(1),
                request_id: Uuid::now_v7(),
            })
            .await
            .expect("complete link")
    }

    #[tokio::test]
    async fn links_exact_identity_and_rejects_callback_replay() {
        let (repository, organization_id, principal) = identity().await;
        let now = Utc::now();
        let flow = flow(
            organization_id,
            OidcFlowPurpose::Link,
            Some(principal.id),
            'a',
            now,
        );
        repository
            .begin_oidc_flow(flow.clone())
            .await
            .expect("begin flow");
        let write = CompleteOidcLinkWrite {
            flow_id: flow.id,
            provider_config_digest: flow.provider_config_digest,
            state_digest: flow.state_digest,
            nonce_digest: flow.nonce_digest,
            pkce_verifier_digest: flow.pkce_verifier_digest,
            subject: ExternalIdentitySubject::parse("subject-42").expect("subject"),
            completed_at: now + Duration::seconds(1),
            request_id: Uuid::now_v7(),
        };
        let link = repository
            .complete_oidc_link(write.clone())
            .await
            .expect("link");
        assert_eq!(link.principal_id, principal.id);
        assert!(matches!(
            repository.complete_oidc_link(write).await,
            Err(RepositoryError::Conflict(_))
        ));
        assert_eq!(
            repository
                .outbox_events()
                .await
                .last()
                .expect("linked event")
                .event_key,
            "identity.external-identity.linked"
        );
    }

    #[tokio::test]
    async fn login_issues_one_ordinary_non_platform_token() {
        let (repository, organization_id, principal) = identity().await;
        let now = Utc::now();
        link_identity(&repository, organization_id, principal.id, now).await;
        let login = flow(organization_id, OidcFlowPurpose::Login, None, '4', now);
        repository
            .begin_oidc_flow(login.clone())
            .await
            .expect("begin login");
        let write = CompleteOidcLoginWrite {
            flow_id: login.id,
            provider_config_digest: login.provider_config_digest,
            state_digest: login.state_digest,
            nonce_digest: login.nonce_digest,
            pkce_verifier_digest: login.pkce_verifier_digest,
            subject: ExternalIdentitySubject::parse("subject-42").expect("subject"),
            token_id: ApiTokenId::new(),
            token_name: ApiTokenName::parse(format!("OIDC {}", login.id)).expect("name"),
            token_digest: ApiTokenDigest::parse(format!("sha256:{}", "9".repeat(64)))
                .expect("digest"),
            completed_at: now + Duration::seconds(2),
            token_expires_at: now + Duration::hours(1),
            request_id: Uuid::now_v7(),
        };
        let token = repository
            .complete_oidc_login(write.clone())
            .await
            .expect("login");
        assert_eq!(token.organization_id, organization_id);
        assert_eq!(token.principal_id, principal.id);
        assert!(matches!(
            repository.complete_oidc_login(write).await,
            Err(RepositoryError::Conflict(_))
        ));
    }
}
