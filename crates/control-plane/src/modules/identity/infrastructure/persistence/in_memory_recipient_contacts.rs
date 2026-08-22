use super::in_memory::{remember, replay, InMemoryIdentityRepository, State};
use crate::modules::identity::domain::entities::{
    IdentityPrincipalKind, RecipientContact, RecipientContactRecord, RecipientContactStatus,
    RecipientContactVerification, RecipientContactVerificationStatus,
};
use crate::modules::identity::domain::events::RecipientContactChanged;
use crate::modules::identity::domain::repositories::{
    BeginRecipientContactVerificationResult, BeginRecipientContactVerificationWrite,
    CompleteRecipientContactVerificationWrite, IRecipientContactRepository,
    ResolvedRecipientContact, RevokeRecipientContactWrite,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, PrincipalId, RecipientContactId, RecipientContactVerificationId,
    RepositoryError,
};
use async_trait::async_trait;

fn authorize_contact_actor(
    state: &State,
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    principal_id: PrincipalId,
) -> Result<(), RepositoryError> {
    let principal = state
        .principals
        .get(&principal_id)
        .filter(|principal| principal.is_active() && principal.kind == IdentityPrincipalKind::Human)
        .ok_or_else(|| {
            RepositoryError::Forbidden(
                "recipient contacts require an active human identity principal".into(),
            )
        })?;
    let membership = state
        .membership_subjects
        .get(&(organization_id, principal.id))
        .and_then(|membership_id| state.memberships.get(membership_id))
        .filter(|membership| membership.is_active());
    if membership.is_none() {
        return Err(RepositoryError::Forbidden(
            "recipient contact actor is not an active organization member".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl IRecipientContactRepository for InMemoryIdentityRepository {
    async fn begin_recipient_contact_verification(
        &self,
        write: BeginRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<BeginRecipientContactVerificationResult>, RepositoryError> {
        let mut state = self.state.write().await;
        authorize_contact_actor(&state, write.organization_id, write.actor_principal_id)?;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        let address_key = (write.actor_principal_id, write.address.as_str().to_owned());
        let contact = match state.recipient_contact_addresses.get(&address_key) {
            Some(contact_id) => state
                .recipient_contacts
                .get(contact_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "recipient contact address index is inconsistent".into(),
                    )
                })?,
            None => RecipientContact::create(
                write.contact_id,
                write.actor_principal_id,
                write.address,
                write.requested_at,
            )
            .map_err(RepositoryError::Storage)?,
        };
        match contact.status {
            RecipientContactStatus::Pending => {}
            RecipientContactStatus::Verified => {
                return Err(RepositoryError::Conflict(
                    "recipient contact is already verified".into(),
                ))
            }
            RecipientContactStatus::Revoked => {
                return Err(RepositoryError::Conflict(
                    "recipient contact is revoked".into(),
                ))
            }
        }
        for verification in state
            .recipient_contact_verifications
            .values_mut()
            .filter(|verification| verification.contact_id == contact.id)
        {
            verification.invalidate(write.requested_at);
        }
        let verification = RecipientContactVerification::create(
            write.verification_id,
            contact.id,
            contact.principal_id,
            contact.address.digest(),
            contact.aggregate_version,
            write.signing_key_id,
            write.requested_at,
            write.expires_at,
        )
        .map_err(RepositoryError::Storage)?;
        let result = BeginRecipientContactVerificationResult {
            contact: contact.record(),
            verification: verification.clone(),
        };
        state
            .recipient_contact_addresses
            .entry(address_key)
            .or_insert(contact.id);
        state.recipient_contacts.insert(contact.id, contact);
        state
            .recipient_contact_verifications
            .insert(verification.id, verification.clone());
        state
            .recipient_contact_verification_organizations
            .insert(verification.id, write.organization_id);
        remember(&mut state, write.idempotency, &result)?;
        state.outbox.push(
            RecipientContactChanged::verification_requested(
                write.organization_id,
                &result.contact,
                &verification,
                write.request_id,
            )
            .map_err(|error| RepositoryError::Storage(error.to_string()))?,
        );
        Ok(IdempotentWrite {
            value: result,
            replayed: false,
        })
    }

    async fn find_recipient_contact(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> Result<Option<RecipientContactRecord>, RepositoryError> {
        let state = self.state.read().await;
        authorize_contact_actor(&state, organization_id, principal_id)?;
        Ok(state
            .recipient_contacts
            .get(&contact_id)
            .filter(|contact| contact.principal_id == principal_id)
            .map(RecipientContact::record))
    }

    async fn list_recipient_contacts(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<RecipientContactRecord>, RepositoryError> {
        let state = self.state.read().await;
        authorize_contact_actor(&state, organization_id, principal_id)?;
        let mut contacts = state
            .recipient_contacts
            .values()
            .filter(|contact| contact.principal_id == principal_id)
            .map(RecipientContact::record)
            .collect::<Vec<_>>();
        contacts.sort_by_key(|contact| (contact.created_at, contact.id));
        Ok(contacts)
    }

    async fn find_recipient_contact_verification(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
        verification_id: RecipientContactVerificationId,
    ) -> Result<Option<RecipientContactVerification>, RepositoryError> {
        let state = self.state.read().await;
        authorize_contact_actor(&state, organization_id, principal_id)?;
        Ok(state
            .recipient_contact_verifications
            .get(&verification_id)
            .filter(|verification| {
                verification.contact_id == contact_id && verification.principal_id == principal_id
            })
            .filter(|verification| {
                state
                    .recipient_contact_verification_organizations
                    .get(&verification.id)
                    == Some(&organization_id)
            })
            .cloned())
    }

    async fn complete_recipient_contact_verification(
        &self,
        write: CompleteRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        authorize_contact_actor(&state, write.organization_id, write.actor_principal_id)?;
        let mut contact = state
            .recipient_contacts
            .get(&write.contact_id)
            .filter(|contact| contact.principal_id == write.actor_principal_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        let mut verification = state
            .recipient_contact_verifications
            .get(&write.claims.challenge_id)
            .filter(|verification| {
                verification.contact_id == contact.id
                    && verification.principal_id == contact.principal_id
                    && verification.claims() == write.claims
            })
            .filter(|verification| {
                state
                    .recipient_contact_verification_organizations
                    .get(&verification.id)
                    == Some(&write.organization_id)
            })
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "recipient contact verification proof does not match an active challenge"
                        .into(),
                )
            })?;
        if verification.status_at(write.completed_at) != RecipientContactVerificationStatus::Pending
        {
            return Err(RepositoryError::Conflict(
                "recipient contact verification is not pending".into(),
            ));
        }
        contact
            .verify(&write.claims, write.completed_at)
            .map_err(RepositoryError::Conflict)?;
        verification
            .consume(write.completed_at)
            .map_err(RepositoryError::Conflict)?;
        let record = contact.record();
        state.recipient_contacts.insert(contact.id, contact);
        state
            .recipient_contact_verifications
            .insert(verification.id, verification.clone());
        remember(&mut state, write.idempotency, &record)?;
        state.outbox.push(
            RecipientContactChanged::verified(
                write.organization_id,
                &record,
                &verification,
                write.request_id,
            )
            .map_err(|error| RepositoryError::Storage(error.to_string()))?,
        );
        Ok(IdempotentWrite {
            value: record,
            replayed: false,
        })
    }

    async fn revoke_recipient_contact(
        &self,
        write: RevokeRecipientContactWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        authorize_contact_actor(&state, write.organization_id, write.actor_principal_id)?;
        let mut contact = state
            .recipient_contacts
            .get(&write.contact_id)
            .filter(|contact| contact.principal_id == write.actor_principal_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        if write.expected_version == 0 || contact.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "recipient contact changed before revocation".into(),
            ));
        }
        if !contact.revoke(write.revoked_at) {
            return Err(RepositoryError::Conflict(
                "recipient contact is already revoked".into(),
            ));
        }
        for verification in state
            .recipient_contact_verifications
            .values_mut()
            .filter(|verification| verification.contact_id == contact.id)
        {
            verification.invalidate(write.revoked_at);
        }
        let record = contact.record();
        state.recipient_contacts.insert(contact.id, contact);
        remember(&mut state, write.idempotency, &record)?;
        state.outbox.push(
            RecipientContactChanged::revoked(write.organization_id, &record, write.request_id)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?,
        );
        Ok(IdempotentWrite {
            value: record,
            replayed: false,
        })
    }

    async fn resolve_verified_recipient_contact(
        &self,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> Result<Option<ResolvedRecipientContact>, RepositoryError> {
        let state = self.state.read().await;
        let principal_active = state
            .principals
            .get(&principal_id)
            .is_some_and(|principal| {
                principal.is_active() && principal.kind == IdentityPrincipalKind::Human
            });
        if !principal_active {
            return Ok(None);
        }
        Ok(state
            .recipient_contacts
            .get(&contact_id)
            .filter(|contact| {
                contact.principal_id == principal_id
                    && contact.status == RecipientContactStatus::Verified
            })
            .and_then(|contact| {
                contact
                    .verified_at
                    .map(|verified_at| ResolvedRecipientContact {
                        id: contact.id,
                        principal_id: contact.principal_id,
                        address: contact.address.clone(),
                        aggregate_version: contact.aggregate_version,
                        verified_at,
                    })
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{IdentityPrincipal, Membership, Organization};
    use crate::modules::identity::domain::value_objects::{
        MembershipRole, OrganizationName, RecipientContactSigningKeyId, RecipientEmailAddress,
    };
    use crate::modules::shared_kernel::domain::{
        IdempotencyRequest, MembershipId, OrganizationId, PrincipalId, RecipientContactId,
        RecipientContactVerificationId, ResourceName,
    };
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    async fn fixture() -> (InMemoryIdentityRepository, OrganizationId, PrincipalId) {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let now = Utc::now();
        let principal = IdentityPrincipal::create(
            principal_id,
            IdentityPrincipalKind::Human,
            ResourceName::parse("Contact owner").expect("name"),
            now,
        );
        let organization = Organization::create(
            organization_id,
            OrganizationName::parse("Contact tenant").expect("organization"),
            now,
        );
        let membership = Membership::create(
            MembershipId::new(),
            organization_id,
            principal_id,
            MembershipRole::Owner,
            now,
        );
        let mut state = repository.state.write().await;
        state.principals.insert(principal_id, principal);
        state.organizations.insert(organization_id, organization);
        state
            .membership_subjects
            .insert((organization_id, principal_id), membership.id);
        state.memberships.insert(membership.id, membership);
        drop(state);
        (repository, organization_id, principal_id)
    }

    fn idempotency(scope: &str, key: &str) -> IdempotencyRequest {
        IdempotencyRequest::new(scope, key, key.as_bytes()).expect("idempotency")
    }

    #[tokio::test]
    async fn reissue_invalidates_old_proof_and_completion_is_exactly_once() {
        let (repository, organization_id, principal_id) = fixture().await;
        let now = Utc::now();
        let address = RecipientEmailAddress::parse("alerts@example.com").expect("address");
        let first = repository
            .begin_recipient_contact_verification(BeginRecipientContactVerificationWrite {
                organization_id,
                actor_principal_id: principal_id,
                contact_id: RecipientContactId::new(),
                verification_id: RecipientContactVerificationId::new(),
                address: address.clone(),
                signing_key_id: RecipientContactSigningKeyId::parse("contact-v1").expect("key"),
                requested_at: now,
                expires_at: now + Duration::minutes(10),
                request_id: Uuid::now_v7(),
                idempotency: idempotency("contacts", "first"),
            })
            .await
            .expect("first challenge");
        let second = repository
            .begin_recipient_contact_verification(BeginRecipientContactVerificationWrite {
                organization_id,
                actor_principal_id: principal_id,
                contact_id: RecipientContactId::new(),
                verification_id: RecipientContactVerificationId::new(),
                address,
                signing_key_id: RecipientContactSigningKeyId::parse("contact-v1").expect("key"),
                requested_at: now + Duration::seconds(1),
                expires_at: now + Duration::minutes(11),
                request_id: Uuid::now_v7(),
                idempotency: idempotency("contacts", "second"),
            })
            .await
            .expect("second challenge");
        assert_eq!(first.value.contact.id, second.value.contact.id);
        assert_eq!(
            repository
                .find_recipient_contact_verification(
                    organization_id,
                    principal_id,
                    first.value.contact.id,
                    first.value.verification.id,
                )
                .await
                .expect("old challenge")
                .expect("old challenge exists")
                .status_at(now + Duration::minutes(1)),
            RecipientContactVerificationStatus::Invalidated
        );
        let stale = repository
            .complete_recipient_contact_verification(CompleteRecipientContactVerificationWrite {
                organization_id,
                actor_principal_id: principal_id,
                contact_id: first.value.contact.id,
                claims: first.value.verification.claims(),
                completed_at: now + Duration::minutes(1),
                request_id: Uuid::now_v7(),
                idempotency: idempotency("contacts/complete", "stale"),
            })
            .await;
        assert!(matches!(stale, Err(RepositoryError::Conflict(_))));

        let complete_idempotency = idempotency("contacts/complete", "current");
        let completed = repository
            .complete_recipient_contact_verification(CompleteRecipientContactVerificationWrite {
                organization_id,
                actor_principal_id: principal_id,
                contact_id: second.value.contact.id,
                claims: second.value.verification.claims(),
                completed_at: now + Duration::minutes(1),
                request_id: Uuid::now_v7(),
                idempotency: complete_idempotency.clone(),
            })
            .await
            .expect("complete");
        assert_eq!(completed.value.status, RecipientContactStatus::Verified);
        let replay = repository
            .complete_recipient_contact_verification(CompleteRecipientContactVerificationWrite {
                organization_id,
                actor_principal_id: principal_id,
                contact_id: second.value.contact.id,
                claims: second.value.verification.claims(),
                completed_at: now + Duration::minutes(1),
                request_id: Uuid::now_v7(),
                idempotency: complete_idempotency,
            })
            .await
            .expect("replay");
        assert!(replay.replayed);
        assert!(repository
            .resolve_verified_recipient_contact(principal_id, second.value.contact.id)
            .await
            .expect("resolve")
            .is_some());
    }

    #[tokio::test]
    async fn service_principals_and_foreign_contacts_fail_closed() {
        let (repository, organization_id, principal_id) = fixture().await;
        let service_id = PrincipalId::new();
        let now = Utc::now();
        let service = IdentityPrincipal::create(
            service_id,
            IdentityPrincipalKind::Service,
            ResourceName::parse("Service").expect("name"),
            now,
        );
        let membership = Membership::create(
            MembershipId::new(),
            organization_id,
            service_id,
            MembershipRole::Member,
            now,
        );
        let mut state = repository.state.write().await;
        state.principals.insert(service_id, service);
        state
            .membership_subjects
            .insert((organization_id, service_id), membership.id);
        state.memberships.insert(membership.id, membership);
        drop(state);
        assert!(matches!(
            repository
                .begin_recipient_contact_verification(BeginRecipientContactVerificationWrite {
                    organization_id,
                    actor_principal_id: service_id,
                    contact_id: RecipientContactId::new(),
                    verification_id: RecipientContactVerificationId::new(),
                    address: RecipientEmailAddress::parse("service@example.com").expect("address"),
                    signing_key_id: RecipientContactSigningKeyId::parse("contact-v1").expect("key"),
                    requested_at: now,
                    expires_at: now + Duration::minutes(10),
                    request_id: Uuid::now_v7(),
                    idempotency: idempotency("contacts", "service"),
                })
                .await,
            Err(RepositoryError::Forbidden(_))
        ));
        assert!(repository
            .find_recipient_contact(organization_id, principal_id, RecipientContactId::new(),)
            .await
            .expect("foreign lookup")
            .is_none());
    }
}
