use super::in_memory::{remember, replay, InMemoryIdentityRepository};
use crate::modules::identity::domain::entities::{
    ExternalIdentityLink, IdentityPrincipalKind,
};
use crate::modules::identity::domain::repositories::{
    IPartnerSubjectLinkRepository, LinkPartnerSubjectWrite, RevokePartnerSubjectLinkWrite,
};
use crate::modules::identity::domain::value_objects::{
    is_partner_provider_key, ExternalIdentitySubject, OidcIssuer,
};
use crate::modules::shared_kernel::domain::{IdempotentWrite, PrincipalId, RepositoryError};
use async_trait::async_trait;

#[async_trait]
impl IPartnerSubjectLinkRepository for InMemoryIdentityRepository {
    async fn link_partner_subject(
        &self,
        write: LinkPartnerSubjectWrite,
    ) -> Result<IdempotentWrite<ExternalIdentityLink>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        if !is_partner_provider_key(&write.link.provider_key) {
            return Err(RepositoryError::Conflict(
                "provider key is not a partner subject link".into(),
            ));
        }
        let active_membership = state
            .membership_subjects
            .get(&(write.organization_id, write.link.principal_id))
            .and_then(|id| state.memberships.get(id))
            .is_some_and(|membership| membership.is_active());
        let principal = state
            .principals
            .get(&write.link.principal_id)
            .filter(|principal| {
                principal.is_active() && principal.kind == IdentityPrincipalKind::Human
            })
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Forbidden(
                    "partner subject link principal is not an active human member".into(),
                )
            })?;
        if !active_membership {
            return Err(RepositoryError::Forbidden(
                "partner subject link principal is not an active organization member".into(),
            ));
        }
        let existing = state
            .external_identity_links
            .values()
            .find(|link| {
                link.issuer == write.link.issuer && link.subject == write.link.subject
            })
            .cloned();
        let link = if let Some(existing) = existing {
            if existing.is_active() {
                if existing.principal_id != write.link.principal_id
                    || existing.provider_key != write.link.provider_key
                {
                    return Err(RepositoryError::Conflict(
                        "partner subject is already bound to another principal".into(),
                    ));
                }
                existing
            } else {
                // Re-bind a revoked (issuer, subject) row to the requested principal.
                let mut rebound = ExternalIdentityLink::create(
                    existing.id,
                    write.link.provider_key.clone(),
                    write.link.issuer.clone(),
                    write.link.subject.clone(),
                    &principal,
                    write.link.created_at,
                )
                .map_err(RepositoryError::Conflict)?;
                rebound.aggregate_version = existing.aggregate_version + 1;
                rebound
            }
        } else {
            if state.external_identity_links.values().any(|link| {
                link.is_active()
                    && link.principal_id == write.link.principal_id
                    && link.issuer == write.link.issuer
            }) {
                return Err(RepositoryError::Conflict(
                    "principal already has an active partner subject for this issuer".into(),
                ));
            }
            write.link.clone()
        };
        state.external_identity_links.insert(link.id, link.clone());
        state.outbox.extend(write.events);
        remember(&mut state, write.idempotency, &link)?;
        let _ = write.actor_principal_id;
        let _ = write.request_id;
        Ok(IdempotentWrite {
            value: link,
            replayed: false,
        })
    }

    async fn revoke_partner_subject_link(
        &self,
        write: RevokePartnerSubjectLinkWrite,
    ) -> Result<IdempotentWrite<ExternalIdentityLink>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        let Some(mut link) = state
            .external_identity_links
            .values()
            .find(|link| {
                link.is_active()
                    && link.provider_key == write.provider_key
                    && link.issuer == write.issuer
                    && link.subject == write.subject
            })
            .cloned()
        else {
            return Err(RepositoryError::NotFound);
        };
        if link.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "partner subject link version conflict".into(),
            ));
        }
        if !link.revoke(write.revoked_at) {
            return Err(RepositoryError::Conflict(
                "partner subject link is already revoked".into(),
            ));
        }
        state.external_identity_links.insert(link.id, link.clone());
        state.outbox.extend(write.events);
        remember(&mut state, write.idempotency, &link)?;
        let _ = write.organization_id;
        let _ = write.actor_principal_id;
        let _ = write.request_id;
        Ok(IdempotentWrite {
            value: link,
            replayed: false,
        })
    }

    async fn find_active_partner_subject_link(
        &self,
        issuer: &OidcIssuer,
        subject: &ExternalIdentitySubject,
    ) -> Result<Option<ExternalIdentityLink>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .external_identity_links
            .values()
            .find(|link| {
                link.is_active()
                    && is_partner_provider_key(&link.provider_key)
                    && &link.issuer == issuer
                    && &link.subject == subject
            })
            .cloned())
    }

    async fn list_active_partner_subject_links_for_principal(
        &self,
        principal_id: PrincipalId,
    ) -> Result<Vec<ExternalIdentityLink>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .external_identity_links
            .values()
            .filter(|link| {
                link.is_active()
                    && is_partner_provider_key(&link.provider_key)
                    && link.principal_id == principal_id
            })
            .cloned()
            .collect())
    }
}
