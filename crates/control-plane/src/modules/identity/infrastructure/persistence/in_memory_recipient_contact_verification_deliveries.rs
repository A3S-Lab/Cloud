use super::in_memory::InMemoryIdentityRepository;
use crate::modules::identity::domain::entities::{
    IdentityPrincipalKind, RecipientContactStatus, RecipientContactVerification,
    RecipientContactVerificationDeliveryFact, RecipientContactVerificationDeliveryOutcome,
    RecipientContactVerificationDeliveryRecord, RecipientContactVerificationDeliveryReservation,
    RecipientContactVerificationDeliveryStatus, RecipientContactVerificationStatus,
};
use crate::modules::identity::domain::repositories::{
    IRecipientContactVerificationDeliveryRepository, RecipientContactVerificationDeliveryAdmission,
    RecipientContactVerificationDispatchStart,
};
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, RecipientContactVerificationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

enum DeliveryAuthority {
    Current {
        verification: RecipientContactVerification,
        address: RecipientEmailAddress,
    },
    Obsolete,
    InvalidFact,
}

fn delivery_authority(
    state: &super::in_memory::State,
    fact: &RecipientContactVerificationDeliveryFact,
    now: DateTime<Utc>,
) -> DeliveryAuthority {
    let Some(stored) = state
        .recipient_contact_verifications
        .get(&fact.verification.id)
    else {
        return DeliveryAuthority::InvalidFact;
    };
    if state
        .recipient_contact_verification_organizations
        .get(&stored.id)
        != Some(&fact.organization_id)
        || stored.claims() != fact.verification.claims()
    {
        return DeliveryAuthority::InvalidFact;
    }
    let principal_active = state
        .principals
        .get(&stored.principal_id)
        .is_some_and(|principal| {
            principal.kind == IdentityPrincipalKind::Human && principal.is_active()
        });
    let membership_active =
        state
            .membership_subjects
            .iter()
            .any(|((organization_id, principal_id), membership_id)| {
                *organization_id == fact.organization_id
                    && *principal_id == stored.principal_id
                    && state
                        .memberships
                        .get(membership_id)
                        .is_some_and(|membership| membership.is_active())
            });
    let Some(contact) = state.recipient_contacts.get(&stored.contact_id) else {
        return DeliveryAuthority::Obsolete;
    };
    if stored.status_at(now) != RecipientContactVerificationStatus::Pending
        || !principal_active
        || !membership_active
        || contact.principal_id != stored.principal_id
        || contact.status != RecipientContactStatus::Pending
        || contact.aggregate_version != stored.contact_version
        || contact.address.digest() != stored.address_digest
    {
        return DeliveryAuthority::Obsolete;
    }
    DeliveryAuthority::Current {
        verification: stored.clone(),
        address: contact.address.clone(),
    }
}

fn obsolete_record(
    verification_id: RecipientContactVerificationId,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    settled_at: DateTime<Utc>,
) -> Result<RecipientContactVerificationDeliveryRecord, RepositoryError> {
    RecipientContactVerificationDeliveryRecord::restore(
        verification_id,
        RecipientContactVerificationDeliveryStatus::Obsolete,
        fence_token,
        reserved_at,
        lease_expires_at,
        None,
        Some(settled_at.max(reserved_at)),
    )
    .map_err(RepositoryError::Storage)
}

#[async_trait]
impl IRecipientContactVerificationDeliveryRepository for InMemoryIdentityRepository {
    async fn reserve_recipient_contact_verification_delivery(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDeliveryAdmission, RepositoryError> {
        if fact.validate().is_err() {
            return Ok(RecipientContactVerificationDeliveryAdmission::InvalidFact);
        }
        let reserved_at = canonical_timestamp(reserved_at);
        let lease_expires_at = canonical_timestamp(lease_expires_at);
        if fence_token.is_nil() || lease_expires_at <= reserved_at {
            return Err(RepositoryError::Storage(
                "recipient contact verification delivery reservation is invalid".into(),
            ));
        }
        let mut state = self.state.write().await;
        if let Some(existing) = state
            .recipient_contact_verification_deliveries
            .get_mut(&fact.id())
        {
            if existing.status.is_terminal() {
                return Ok(RecipientContactVerificationDeliveryAdmission::Terminal(
                    existing.status,
                ));
            }
            if existing.status == RecipientContactVerificationDeliveryStatus::Dispatching {
                existing.status = RecipientContactVerificationDeliveryStatus::Indeterminate;
                existing.settled_at = Some(
                    existing
                        .dispatch_started_at
                        .unwrap_or(reserved_at)
                        .max(reserved_at),
                );
                existing.validate().map_err(RepositoryError::Storage)?;
                return Ok(RecipientContactVerificationDeliveryAdmission::Terminal(
                    existing.status,
                ));
            }
            if reserved_at < existing.lease_expires_at {
                return Ok(RecipientContactVerificationDeliveryAdmission::Deferred {
                    lease_expires_at: existing.lease_expires_at,
                });
            }
        }

        match delivery_authority(&state, fact, reserved_at) {
            DeliveryAuthority::InvalidFact => {
                Ok(RecipientContactVerificationDeliveryAdmission::InvalidFact)
            }
            DeliveryAuthority::Obsolete => {
                let record = obsolete_record(
                    fact.id(),
                    fence_token,
                    reserved_at,
                    lease_expires_at,
                    reserved_at,
                )?;
                state
                    .recipient_contact_verification_deliveries
                    .insert(fact.id(), record);
                Ok(RecipientContactVerificationDeliveryAdmission::Terminal(
                    RecipientContactVerificationDeliveryStatus::Obsolete,
                ))
            }
            DeliveryAuthority::Current {
                verification,
                address,
            } => {
                let record = RecipientContactVerificationDeliveryRecord::restore(
                    fact.id(),
                    RecipientContactVerificationDeliveryStatus::Reserved,
                    fence_token,
                    reserved_at,
                    lease_expires_at,
                    None,
                    None,
                )
                .map_err(RepositoryError::Storage)?;
                state
                    .recipient_contact_verification_deliveries
                    .insert(fact.id(), record);
                Ok(RecipientContactVerificationDeliveryAdmission::Reserved(
                    RecipientContactVerificationDeliveryReservation {
                        verification,
                        address,
                        fence_token,
                        lease_expires_at,
                    },
                ))
            }
        }
    }

    async fn start_recipient_contact_verification_dispatch(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
        fence_token: Uuid,
        started_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDispatchStart, RepositoryError> {
        let started_at = canonical_timestamp(started_at);
        let mut state = self.state.write().await;
        let Some(snapshot) = state
            .recipient_contact_verification_deliveries
            .get(&fact.id())
            .cloned()
        else {
            return Ok(RecipientContactVerificationDispatchStart::Deferred);
        };
        if snapshot.status.is_terminal() {
            return Ok(RecipientContactVerificationDispatchStart::Terminal(
                snapshot.status,
            ));
        }
        if snapshot.status == RecipientContactVerificationDeliveryStatus::Dispatching {
            let record = state
                .recipient_contact_verification_deliveries
                .get_mut(&fact.id())
                .ok_or_else(|| RepositoryError::Storage("delivery disappeared".into()))?;
            record.status = RecipientContactVerificationDeliveryStatus::Indeterminate;
            record.settled_at = Some(
                record
                    .dispatch_started_at
                    .unwrap_or(started_at)
                    .max(started_at),
            );
            record.validate().map_err(RepositoryError::Storage)?;
            return Ok(RecipientContactVerificationDispatchStart::Terminal(
                record.status,
            ));
        }
        if snapshot.fence_token != fence_token || started_at >= snapshot.lease_expires_at {
            return Ok(RecipientContactVerificationDispatchStart::Deferred);
        }
        match delivery_authority(&state, fact, started_at) {
            DeliveryAuthority::InvalidFact => {
                return Ok(RecipientContactVerificationDispatchStart::Deferred)
            }
            DeliveryAuthority::Obsolete => {
                let record = state
                    .recipient_contact_verification_deliveries
                    .get_mut(&fact.id())
                    .ok_or_else(|| RepositoryError::Storage("delivery disappeared".into()))?;
                record.status = RecipientContactVerificationDeliveryStatus::Obsolete;
                record.settled_at = Some(started_at.max(record.reserved_at));
                record.validate().map_err(RepositoryError::Storage)?;
                return Ok(RecipientContactVerificationDispatchStart::Terminal(
                    record.status,
                ));
            }
            DeliveryAuthority::Current { .. } => {}
        }
        let record = state
            .recipient_contact_verification_deliveries
            .get_mut(&fact.id())
            .ok_or_else(|| RepositoryError::Storage("delivery disappeared".into()))?;
        record.status = RecipientContactVerificationDeliveryStatus::Dispatching;
        record.dispatch_started_at = Some(started_at.max(record.reserved_at));
        record.validate().map_err(RepositoryError::Storage)?;
        Ok(RecipientContactVerificationDispatchStart::Authorized)
    }

    async fn settle_recipient_contact_verification_delivery(
        &self,
        verification_id: RecipientContactVerificationId,
        fence_token: Uuid,
        outcome: RecipientContactVerificationDeliveryOutcome,
        settled_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDeliveryRecord, RepositoryError> {
        let settled_at = canonical_timestamp(settled_at);
        let mut state = self.state.write().await;
        let record = state
            .recipient_contact_verification_deliveries
            .get_mut(&verification_id)
            .ok_or(RepositoryError::NotFound)?;
        if record.status == outcome.status() && record.fence_token == fence_token {
            return Ok(record.clone());
        }
        if record.status != RecipientContactVerificationDeliveryStatus::Dispatching
            || record.fence_token != fence_token
        {
            return Err(RepositoryError::Conflict(
                "recipient contact verification delivery crossed its dispatch fence".into(),
            ));
        }
        let dispatch_started_at = record.dispatch_started_at.ok_or_else(|| {
            RepositoryError::Storage(
                "recipient contact verification delivery is missing its dispatch time".into(),
            )
        })?;
        record.status = outcome.status();
        record.settled_at = Some(settled_at.max(dispatch_started_at));
        record.validate().map_err(RepositoryError::Storage)?;
        Ok(record.clone())
    }

    async fn find_recipient_contact_verification_delivery(
        &self,
        verification_id: RecipientContactVerificationId,
    ) -> Result<Option<RecipientContactVerificationDeliveryRecord>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .recipient_contact_verification_deliveries
            .get(&verification_id)
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{
        IdentityPrincipal, Membership, Organization, RecipientContact,
    };
    use crate::modules::identity::domain::value_objects::{
        MembershipRole, OrganizationName, RecipientContactSigningKeyId,
    };
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, MembershipId, OrganizationId, PrincipalId, RecipientContactId,
        ResourceName,
    };
    use chrono::Duration;

    struct Fixture {
        repository: InMemoryIdentityRepository,
        fact: RecipientContactVerificationDeliveryFact,
        membership_id: MembershipId,
        now: DateTime<Utc>,
    }

    async fn fixture() -> Fixture {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let membership_id = MembershipId::new();
        let now = canonical_timestamp(Utc::now());
        let principal = IdentityPrincipal::create(
            principal_id,
            IdentityPrincipalKind::Human,
            ResourceName::parse("Recipient contact owner").expect("principal name"),
            now,
        );
        let organization = Organization::create(
            organization_id,
            OrganizationName::parse("Recipient contact tenant").expect("organization name"),
            now,
        );
        let membership = Membership::create(
            membership_id,
            organization_id,
            principal_id,
            MembershipRole::Member,
            now,
        );
        let contact = RecipientContact::create(
            RecipientContactId::new(),
            principal_id,
            RecipientEmailAddress::parse("private@example.test").expect("recipient address"),
            now,
        )
        .expect("recipient contact");
        let verification = RecipientContactVerification::create(
            RecipientContactVerificationId::new(),
            contact.id,
            principal_id,
            contact.address.digest(),
            contact.aggregate_version,
            RecipientContactSigningKeyId::parse("contact-v1").expect("signing key"),
            now,
            now + Duration::minutes(10),
        )
        .expect("verification");
        let fact = RecipientContactVerificationDeliveryFact {
            organization_id,
            verification: verification.clone(),
        };
        let mut state = repository.state.write().await;
        state.organizations.insert(organization_id, organization);
        state.principals.insert(principal_id, principal);
        state
            .membership_subjects
            .insert((organization_id, principal_id), membership_id);
        state.memberships.insert(membership_id, membership);
        state.recipient_contacts.insert(contact.id, contact);
        state
            .recipient_contact_verifications
            .insert(verification.id, verification.clone());
        state
            .recipient_contact_verification_organizations
            .insert(verification.id, organization_id);
        drop(state);
        Fixture {
            repository,
            fact,
            membership_id,
            now,
        }
    }

    #[tokio::test]
    async fn dispatch_fence_is_single_use_and_replay_becomes_indeterminate() {
        let fixture = fixture().await;
        let fence = Uuid::now_v7();
        let reservation = match fixture
            .repository
            .reserve_recipient_contact_verification_delivery(
                &fixture.fact,
                fence,
                fixture.now,
                fixture.now + Duration::minutes(1),
            )
            .await
            .expect("reservation")
        {
            RecipientContactVerificationDeliveryAdmission::Reserved(value) => value,
            other => panic!("expected reservation, got {other:?}"),
        };
        assert_eq!(reservation.fence_token, fence);
        assert!(!format!("{reservation:?}").contains("private@example.test"));

        assert!(matches!(
            fixture
                .repository
                .reserve_recipient_contact_verification_delivery(
                    &fixture.fact,
                    Uuid::now_v7(),
                    fixture.now + Duration::seconds(1),
                    fixture.now + Duration::seconds(61),
                )
                .await
                .expect("competing reservation"),
            RecipientContactVerificationDeliveryAdmission::Deferred { .. }
        ));
        assert_eq!(
            fixture
                .repository
                .start_recipient_contact_verification_dispatch(
                    &fixture.fact,
                    Uuid::now_v7(),
                    fixture.now + Duration::seconds(2),
                )
                .await
                .expect("wrong fence"),
            RecipientContactVerificationDispatchStart::Deferred
        );
        assert_eq!(
            fixture
                .repository
                .start_recipient_contact_verification_dispatch(
                    &fixture.fact,
                    fence,
                    fixture.now + Duration::seconds(2),
                )
                .await
                .expect("dispatch start"),
            RecipientContactVerificationDispatchStart::Authorized
        );

        assert_eq!(
            fixture
                .repository
                .reserve_recipient_contact_verification_delivery(
                    &fixture.fact,
                    Uuid::now_v7(),
                    fixture.now + Duration::seconds(3),
                    fixture.now + Duration::seconds(63),
                )
                .await
                .expect("post-fence replay"),
            RecipientContactVerificationDeliveryAdmission::Terminal(
                RecipientContactVerificationDeliveryStatus::Indeterminate
            )
        );
        let replayed_settlement = fixture
            .repository
            .settle_recipient_contact_verification_delivery(
                fixture.fact.id(),
                fence,
                RecipientContactVerificationDeliveryOutcome::Indeterminate,
                fixture.now + Duration::seconds(4),
            )
            .await
            .expect("idempotent indeterminate settlement");
        assert_eq!(
            replayed_settlement.status,
            RecipientContactVerificationDeliveryStatus::Indeterminate
        );
        assert!(matches!(
            fixture
                .repository
                .settle_recipient_contact_verification_delivery(
                    fixture.fact.id(),
                    fence,
                    RecipientContactVerificationDeliveryOutcome::Delivered,
                    fixture.now + Duration::seconds(5),
                )
                .await,
            Err(RepositoryError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn expired_reservations_can_be_refenced_but_authority_drift_is_terminal() {
        let fixture = fixture().await;
        let first_fence = Uuid::now_v7();
        fixture
            .repository
            .reserve_recipient_contact_verification_delivery(
                &fixture.fact,
                first_fence,
                fixture.now,
                fixture.now + Duration::minutes(1),
            )
            .await
            .expect("first reservation");
        let second_fence = Uuid::now_v7();
        let renewed = fixture
            .repository
            .reserve_recipient_contact_verification_delivery(
                &fixture.fact,
                second_fence,
                fixture.now + Duration::minutes(1),
                fixture.now + Duration::minutes(2),
            )
            .await
            .expect("expired reservation renewal");
        assert!(matches!(
            renewed,
            RecipientContactVerificationDeliveryAdmission::Reserved(
                RecipientContactVerificationDeliveryReservation {
                    fence_token,
                    ..
                }
            ) if fence_token == second_fence
        ));

        let mut state = fixture.repository.state.write().await;
        state
            .memberships
            .get_mut(&fixture.membership_id)
            .expect("membership")
            .revoke(fixture.now + Duration::seconds(61));
        drop(state);
        assert_eq!(
            fixture
                .repository
                .start_recipient_contact_verification_dispatch(
                    &fixture.fact,
                    second_fence,
                    fixture.now + Duration::seconds(62),
                )
                .await
                .expect("authority recheck"),
            RecipientContactVerificationDispatchStart::Terminal(
                RecipientContactVerificationDeliveryStatus::Obsolete
            )
        );
    }

    #[tokio::test]
    async fn invalidated_expired_and_drifted_facts_never_reserve_dispatch() {
        let invalidated = fixture().await;
        invalidated
            .repository
            .state
            .write()
            .await
            .recipient_contact_verifications
            .get_mut(&invalidated.fact.id())
            .expect("verification")
            .invalidate(invalidated.now + Duration::seconds(1));
        assert_eq!(
            invalidated
                .repository
                .reserve_recipient_contact_verification_delivery(
                    &invalidated.fact,
                    Uuid::now_v7(),
                    invalidated.now + Duration::seconds(2),
                    invalidated.now + Duration::seconds(62),
                )
                .await
                .expect("invalidated fact"),
            RecipientContactVerificationDeliveryAdmission::Terminal(
                RecipientContactVerificationDeliveryStatus::Obsolete
            )
        );

        let expired = fixture().await;
        assert_eq!(
            expired
                .repository
                .reserve_recipient_contact_verification_delivery(
                    &expired.fact,
                    Uuid::now_v7(),
                    expired.fact.verification.expires_at,
                    expired.fact.verification.expires_at + Duration::minutes(1),
                )
                .await
                .expect("expired fact"),
            RecipientContactVerificationDeliveryAdmission::Terminal(
                RecipientContactVerificationDeliveryStatus::Obsolete
            )
        );

        let drifted = fixture().await;
        let mut drifted_fact = drifted.fact.clone();
        drifted_fact.verification.contact_version += 1;
        assert_eq!(
            drifted
                .repository
                .reserve_recipient_contact_verification_delivery(
                    &drifted_fact,
                    Uuid::now_v7(),
                    drifted.now,
                    drifted.now + Duration::minutes(1),
                )
                .await
                .expect("drifted fact"),
            RecipientContactVerificationDeliveryAdmission::InvalidFact
        );
        assert!(drifted
            .repository
            .find_recipient_contact_verification_delivery(drifted.fact.id())
            .await
            .expect("delivery lookup")
            .is_none());
    }
}
