use super::*;
use a3s_cloud_control_plane::modules::identity::domain::entities::{
    RecipientContactStatus, RecipientContactVerificationDeliveryFact,
    RecipientContactVerificationDeliveryOutcome, RecipientContactVerificationDeliveryStatus,
    RecipientContactVerificationStatus,
};
use a3s_cloud_control_plane::modules::identity::domain::repositories::{
    BeginRecipientContactVerificationWrite, CompleteRecipientContactVerificationWrite,
    IRecipientContactRepository, IRecipientContactVerificationDeliveryRepository,
    RecipientContactVerificationDeliveryAdmission, RecipientContactVerificationDispatchStart,
    RevokeRecipientContactWrite,
};
use a3s_cloud_control_plane::modules::identity::domain::services::IRecipientContactProofService;
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    RecipientContactSigningKeyId, RecipientEmailAddress,
};
use a3s_cloud_control_plane::modules::identity::infrastructure::HmacRecipientContactProofService;
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, RecipientContactId,
    RecipientContactVerificationId, RepositoryError,
};
use zeroize::Zeroizing;

const IDEMPOTENCY_SCOPE: &str = "tests/recipient-contacts";
const DELIVERY_IDEMPOTENCY_SCOPE: &str = "tests/recipient-contact-deliveries";

fn idempotency(key: &str) -> IdempotencyRequest {
    IdempotencyRequest::new(IDEMPOTENCY_SCOPE, key, key.as_bytes())
        .expect("recipient-contact test idempotency")
}

fn delivery_idempotency(key: &str) -> IdempotencyRequest {
    IdempotencyRequest::new(DELIVERY_IDEMPOTENCY_SCOPE, key, key.as_bytes())
        .expect("recipient-contact delivery test idempotency")
}

async fn seed_identity_authority(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    human_id: PrincipalId,
    other_human_id: PrincipalId,
    service_id: PrincipalId,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Recipient contact tenant', ")
            .bind(format!("recipient-contact-{organization_id}"))
            .append(", 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    for (principal_id, kind, name) in [
        (human_id, "human", "Recipient contact owner"),
        (other_human_id, "human", "Other recipient contact human"),
        (service_id, "service", "Recipient contact service"),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
                )
                .bind(principal_id.as_uuid())
                .append(", ")
                .bind(kind)
                .append(", ")
                .bind(name)
                .append(", 1, ")
                .bind(now)
                .append(", null)"),
            )
            .await?;
        database
            .execute(
                sql_query::<()>(
                    "insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (",
                )
                .bind(Uuid::now_v7())
                .append(", ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(principal_id.as_uuid())
                .append(", 'member', 1, ")
                .bind(now)
                .append(", ")
                .bind(now)
                .append(", null)"),
            )
            .await?;
    }
    Ok(())
}

pub async fn exercise_recipient_contact_persistence(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let repository = PostgresIdentityRepository::new(executor.clone());
    let organization_id = OrganizationId::new();
    let principal_id = PrincipalId::new();
    let other_principal_id = PrincipalId::new();
    let service_principal_id = PrincipalId::new();
    seed_identity_authority(
        &database,
        organization_id,
        principal_id,
        other_principal_id,
        service_principal_id,
    )
    .await?;

    let address = RecipientEmailAddress::parse("Owner.Alert+Prod@Example.COM")?;
    let canonical_address = address.as_str().to_owned();
    let signing_key_id = RecipientContactSigningKeyId::parse("recipient-contact-v1")?;
    let proof_service = HmacRecipientContactProofService::new(
        signing_key_id.clone(),
        Zeroizing::new(vec![0x5a; 32]),
    )?;
    let requested_at = Utc::now();
    let first_write = BeginRecipientContactVerificationWrite {
        organization_id,
        actor_principal_id: principal_id,
        contact_id: RecipientContactId::new(),
        verification_id: RecipientContactVerificationId::new(),
        address: address.clone(),
        signing_key_id: signing_key_id.clone(),
        requested_at,
        expires_at: requested_at + chrono::Duration::minutes(10),
        request_id: Uuid::now_v7(),
        idempotency: idempotency("begin-first"),
    };
    let first = repository
        .begin_recipient_contact_verification(first_write.clone())
        .await?;
    assert!(!first.replayed);
    assert_eq!(first.value.contact.status, RecipientContactStatus::Pending);
    assert_eq!(first.value.contact.address_hint, "***@example.com");
    assert!(!serde_json::to_string(&first.value)?.contains(&canonical_address));
    let replayed_first = repository
        .begin_recipient_contact_verification(first_write)
        .await?;
    assert!(replayed_first.replayed);
    assert_eq!(replayed_first.value, first.value);
    let first_proof = proof_service.issue(&first.value.verification).await?;

    let second_requested_at = first.value.verification.issued_at + chrono::Duration::seconds(1);
    let second = repository
        .begin_recipient_contact_verification(BeginRecipientContactVerificationWrite {
            organization_id,
            actor_principal_id: principal_id,
            contact_id: RecipientContactId::new(),
            verification_id: RecipientContactVerificationId::new(),
            address,
            signing_key_id: signing_key_id.clone(),
            requested_at: second_requested_at,
            expires_at: second_requested_at + chrono::Duration::minutes(10),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("begin-second"),
        })
        .await?;
    assert_eq!(second.value.contact.id, first.value.contact.id);
    assert_ne!(second.value.verification.id, first.value.verification.id);
    let invalidated = repository
        .find_recipient_contact_verification(
            organization_id,
            principal_id,
            first.value.contact.id,
            first.value.verification.id,
        )
        .await?
        .expect("invalidated first challenge");
    assert_eq!(
        invalidated.status_at(second_requested_at),
        RecipientContactVerificationStatus::Invalidated
    );
    let obsolete_fence = Uuid::now_v7();
    let obsolete_fact = RecipientContactVerificationDeliveryFact {
        organization_id,
        verification: first.value.verification.clone(),
    };
    assert_eq!(
        repository
            .reserve_recipient_contact_verification_delivery(
                &obsolete_fact,
                obsolete_fence,
                second_requested_at + chrono::Duration::milliseconds(100),
                second_requested_at + chrono::Duration::seconds(60),
            )
            .await?,
        RecipientContactVerificationDeliveryAdmission::Terminal(
            RecipientContactVerificationDeliveryStatus::Obsolete
        )
    );
    assert_eq!(
        repository
            .find_recipient_contact_verification_delivery(first.value.verification.id)
            .await?
            .expect("obsolete delivery evidence")
            .status,
        RecipientContactVerificationDeliveryStatus::Obsolete
    );
    let stale_claims = proof_service
        .verify(
            &first_proof,
            first.value.verification.issued_at + chrono::Duration::seconds(2),
        )
        .await?;
    let stale_completion = repository
        .complete_recipient_contact_verification(CompleteRecipientContactVerificationWrite {
            organization_id,
            actor_principal_id: principal_id,
            contact_id: first.value.contact.id,
            claims: stale_claims,
            completed_at: second_requested_at + chrono::Duration::seconds(1),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("complete-invalidated"),
        })
        .await;
    assert!(matches!(
        stale_completion,
        Err(RepositoryError::Conflict(_))
    ));

    assert!(repository
        .find_recipient_contact(organization_id, other_principal_id, first.value.contact.id,)
        .await?
        .is_none());
    assert!(repository
        .find_recipient_contact_verification(
            organization_id,
            other_principal_id,
            first.value.contact.id,
            second.value.verification.id,
        )
        .await?
        .is_none());
    let service_begin = repository
        .begin_recipient_contact_verification(BeginRecipientContactVerificationWrite {
            organization_id,
            actor_principal_id: service_principal_id,
            contact_id: RecipientContactId::new(),
            verification_id: RecipientContactVerificationId::new(),
            address: RecipientEmailAddress::parse("service@example.com")?,
            signing_key_id: signing_key_id.clone(),
            requested_at: second_requested_at,
            expires_at: second_requested_at + chrono::Duration::minutes(10),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("service-begin"),
        })
        .await;
    assert!(matches!(service_begin, Err(RepositoryError::Forbidden(_))));

    let second_delivery_fact = RecipientContactVerificationDeliveryFact {
        organization_id,
        verification: second.value.verification.clone(),
    };
    let second_delivery_fence = Uuid::now_v7();
    let second_delivery_reserved_at =
        second.value.verification.issued_at + chrono::Duration::milliseconds(100);
    let second_delivery = repository
        .reserve_recipient_contact_verification_delivery(
            &second_delivery_fact,
            second_delivery_fence,
            second_delivery_reserved_at,
            second_delivery_reserved_at + chrono::Duration::minutes(1),
        )
        .await?;
    let reservation = match second_delivery {
        RecipientContactVerificationDeliveryAdmission::Reserved(value) => value,
        other => return Err(format!("expected delivery reservation, got {other:?}").into()),
    };
    assert_eq!(reservation.fence_token, second_delivery_fence);
    assert_eq!(reservation.address.as_str(), canonical_address);
    assert!(!format!("{reservation:?}").contains(&canonical_address));
    assert_eq!(
        repository
            .start_recipient_contact_verification_dispatch(
                &second_delivery_fact,
                second_delivery_fence,
                second_delivery_reserved_at + chrono::Duration::milliseconds(100),
            )
            .await?,
        RecipientContactVerificationDispatchStart::Authorized
    );

    let restarted_repository = PostgresIdentityRepository::new(executor.clone());
    assert_eq!(
        restarted_repository
            .reserve_recipient_contact_verification_delivery(
                &second_delivery_fact,
                Uuid::now_v7(),
                second_delivery_reserved_at + chrono::Duration::milliseconds(200),
                second_delivery_reserved_at + chrono::Duration::seconds(61),
            )
            .await?,
        RecipientContactVerificationDeliveryAdmission::Terminal(
            RecipientContactVerificationDeliveryStatus::Indeterminate
        )
    );
    assert_eq!(
        restarted_repository
            .settle_recipient_contact_verification_delivery(
                second.value.verification.id,
                second_delivery_fence,
                RecipientContactVerificationDeliveryOutcome::Indeterminate,
                second_delivery_reserved_at + chrono::Duration::milliseconds(300),
            )
            .await?
            .status,
        RecipientContactVerificationDeliveryStatus::Indeterminate
    );
    assert!(matches!(
        restarted_repository
            .settle_recipient_contact_verification_delivery(
                second.value.verification.id,
                second_delivery_fence,
                RecipientContactVerificationDeliveryOutcome::Delivered,
                second_delivery_reserved_at + chrono::Duration::milliseconds(400),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let delivered_requested_at = second_requested_at + chrono::Duration::seconds(10);
    let delivered = repository
        .begin_recipient_contact_verification(BeginRecipientContactVerificationWrite {
            organization_id,
            actor_principal_id: other_principal_id,
            contact_id: RecipientContactId::new(),
            verification_id: RecipientContactVerificationId::new(),
            address: RecipientEmailAddress::parse("delivery-target@example.com")?,
            signing_key_id: signing_key_id.clone(),
            requested_at: delivered_requested_at,
            expires_at: delivered_requested_at + chrono::Duration::minutes(10),
            request_id: Uuid::now_v7(),
            idempotency: delivery_idempotency("begin-delivered"),
        })
        .await?;
    let delivered_fact = RecipientContactVerificationDeliveryFact {
        organization_id,
        verification: delivered.value.verification.clone(),
    };
    let delivered_fence = Uuid::now_v7();
    let delivered_reserved_at =
        delivered.value.verification.issued_at + chrono::Duration::milliseconds(100);
    assert!(matches!(
        repository
            .reserve_recipient_contact_verification_delivery(
                &delivered_fact,
                delivered_fence,
                delivered_reserved_at,
                delivered_reserved_at + chrono::Duration::minutes(1),
            )
            .await?,
        RecipientContactVerificationDeliveryAdmission::Reserved(_)
    ));
    assert_eq!(
        repository
            .start_recipient_contact_verification_dispatch(
                &delivered_fact,
                delivered_fence,
                delivered_reserved_at + chrono::Duration::milliseconds(100),
            )
            .await?,
        RecipientContactVerificationDispatchStart::Authorized
    );
    assert_eq!(
        repository
            .settle_recipient_contact_verification_delivery(
                delivered.value.verification.id,
                delivered_fence,
                RecipientContactVerificationDeliveryOutcome::Delivered,
                delivered_reserved_at + chrono::Duration::milliseconds(200),
            )
            .await?
            .status,
        RecipientContactVerificationDeliveryStatus::Delivered
    );
    assert_eq!(
        restarted_repository
            .reserve_recipient_contact_verification_delivery(
                &delivered_fact,
                Uuid::now_v7(),
                delivered_reserved_at + chrono::Duration::milliseconds(300),
                delivered_reserved_at + chrono::Duration::seconds(61),
            )
            .await?,
        RecipientContactVerificationDeliveryAdmission::Terminal(
            RecipientContactVerificationDeliveryStatus::Delivered
        )
    );

    let second_proof = proof_service.issue(&second.value.verification).await?;
    let completed_at = second.value.verification.issued_at + chrono::Duration::minutes(1);
    let complete_write = CompleteRecipientContactVerificationWrite {
        organization_id,
        actor_principal_id: principal_id,
        contact_id: second.value.contact.id,
        claims: proof_service.verify(&second_proof, completed_at).await?,
        completed_at,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("complete-second"),
    };
    let completed = repository
        .complete_recipient_contact_verification(complete_write.clone())
        .await?;
    assert_eq!(completed.value.status, RecipientContactStatus::Verified);
    assert_eq!(completed.value.aggregate_version, 2);
    let replayed_completion = repository
        .complete_recipient_contact_verification(complete_write.clone())
        .await?;
    assert!(replayed_completion.replayed);
    assert_eq!(replayed_completion.value, completed.value);
    let consumed_reuse = repository
        .complete_recipient_contact_verification(CompleteRecipientContactVerificationWrite {
            idempotency: idempotency("complete-consumed"),
            request_id: Uuid::now_v7(),
            ..complete_write
        })
        .await;
    assert!(matches!(consumed_reuse, Err(RepositoryError::Conflict(_))));
    let resolved = repository
        .resolve_verified_recipient_contact(principal_id, completed.value.id)
        .await?
        .expect("verified internal recipient resolution");
    assert_eq!(resolved.address.as_str(), canonical_address);
    assert!(repository
        .resolve_verified_recipient_contact(other_principal_id, completed.value.id)
        .await?
        .is_none());

    assert!(database
        .execute(
            sql_query::<()>("update recipient_contacts set canonical_address = ")
                .bind("changed@example.com")
                .append(" where id = ")
                .bind(completed.value.id.as_uuid()),
        )
        .await
        .is_err());
    assert!(database
        .execute(
            sql_query::<()>(
                "update recipient_contact_verification_deliveries set state = 'reserved', dispatch_started_at = null, settled_at = null where verification_id = ",
            )
            .bind(delivered.value.verification.id.as_uuid()),
        )
        .await
        .is_err());
    assert!(database
        .execute(
            sql_query::<()>(
                "delete from recipient_contact_verification_deliveries where verification_id = ",
            )
            .bind(delivered.value.verification.id.as_uuid()),
        )
        .await
        .is_err());
    assert!(database
        .execute(
            sql_query::<()>("delete from recipient_contacts where id = ")
                .bind(completed.value.id.as_uuid()),
        )
        .await
        .is_err());
    assert!(database
        .execute(
            sql_query::<()>("delete from recipient_contact_verifications where id = ")
                .bind(second.value.verification.id.as_uuid()),
        )
        .await
        .is_err());
    let service_address = RecipientEmailAddress::parse("raw-service@example.com")?;
    assert!(database
        .execute(
            sql_query::<()>(
                "insert into recipient_contacts (id, principal_id, canonical_address, address_digest, aggregate_version, state, created_at, updated_at, verified_at, revoked_at) values (",
            )
            .bind(RecipientContactId::new().as_uuid())
            .append(", ")
            .bind(service_principal_id.as_uuid())
            .append(", ")
            .bind(service_address.as_str())
            .append(", ")
            .bind(service_address.digest().as_str())
            .append(", 1, 'pending', ")
            .bind(Utc::now())
            .append(", ")
            .bind(Utc::now())
            .append(", null, null)"),
        )
        .await
        .is_err());

    let stale_revocation = repository
        .revoke_recipient_contact(RevokeRecipientContactWrite {
            organization_id,
            actor_principal_id: principal_id,
            contact_id: completed.value.id,
            expected_version: 1,
            revoked_at: completed_at + chrono::Duration::minutes(1),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("revoke-stale"),
        })
        .await;
    assert!(matches!(
        stale_revocation,
        Err(RepositoryError::Conflict(_))
    ));
    let revoke_write = RevokeRecipientContactWrite {
        organization_id,
        actor_principal_id: principal_id,
        contact_id: completed.value.id,
        expected_version: 2,
        revoked_at: completed_at + chrono::Duration::minutes(1),
        request_id: Uuid::now_v7(),
        idempotency: idempotency("revoke-current"),
    };
    let revoked = repository
        .revoke_recipient_contact(revoke_write.clone())
        .await?;
    assert_eq!(revoked.value.status, RecipientContactStatus::Revoked);
    assert_eq!(revoked.value.aggregate_version, 3);
    assert!(
        repository
            .revoke_recipient_contact(revoke_write)
            .await?
            .replayed
    );
    assert!(repository
        .resolve_verified_recipient_contact(principal_id, completed.value.id)
        .await?
        .is_none());

    let mailbox_pattern = format!("%{canonical_address}%");
    let proof_pattern = format!("%{}%", second_proof.as_str());
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from recipient_contacts where id = ",
            )
            .bind(completed.value.id.as_uuid())
            .append(" and canonical_address = ")
            .bind(canonical_address.as_str())
            .append("), (select count(*) from outbox_events where aggregate_id = ")
            .bind(completed.value.id.as_uuid())
            .append("), (select count(*) from audit_records where aggregate_id = ")
            .bind(completed.value.id.as_uuid())
            .append("), (select count(*) from idempotency_records where scope_key = ")
            .bind(IDEMPOTENCY_SCOPE)
            .append("), (select count(*) from outbox_events where payload::text like ")
            .bind(mailbox_pattern.as_str())
            .append("), (select count(*) from audit_records where details::text like ")
            .bind(mailbox_pattern.as_str())
            .append("), (select count(*) from idempotency_records where response::text like ")
            .bind(mailbox_pattern.as_str())
            .append("), (select count(*) from (select payload::text as material from outbox_events union all select details::text from audit_records union all select response::text from idempotency_records union all select to_jsonb(recipient_contact_verifications)::text from recipient_contact_verifications union all select to_jsonb(recipient_contact_verification_deliveries)::text from recipient_contact_verification_deliveries) as persisted_material where material like ")
            .bind(proof_pattern.as_str())
            .append(")"),
        )
        .await?;
    assert_eq!(evidence, (1, 4, 4, 4, 0, 0, 0, 0));
    let delivery_target_pattern = "%delivery-target@example.com%";
    let delivery_evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>(
                "select count(*), count(*) filter (where to_jsonb(recipient_contact_verification_deliveries)::text like ",
            )
            .bind(delivery_target_pattern)
            .append("), count(*) filter (where to_jsonb(recipient_contact_verification_deliveries)::text like ")
            .bind(proof_pattern.as_str())
            .append(") from recipient_contact_verification_deliveries"),
        )
        .await?;
    assert_eq!(delivery_evidence, (3, 0, 0));
    Ok(())
}
