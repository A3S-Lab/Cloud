use super::postgres::PostgresIdentityRepository;
use super::postgres_inference_credentials::{
    credential_query, insert_credential, update_credential_row, InferenceCredentialRow,
};
use super::postgres_inference_credentials_schema::InferenceCredentialDeliveryReceipts;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation, require_one_row,
    store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    InferenceCredential, InferenceCredentialDeliveryReceipt,
};
use crate::modules::identity::domain::repositories::{
    CreateInferenceCredentialWrite, IInferenceCredentialLifecycleRepository,
    InferenceCredentialWrite, InferenceCredentialWriteReference, RotateInferenceCredentialWrite,
    RevokeInferenceCredentialWrite,
};
use crate::modules::secrets::domain::EncryptedSecretValue;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotencyRequest, OrganizationId, RepositoryError,
};
use a3s_orm::{delete_from, insert_into, select_from, OrderDirection, PostgresTransaction};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[async_trait]
impl IInferenceCredentialLifecycleRepository for PostgresIdentityRepository {
    async fn replay_inference_credential_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<InferenceCredentialWrite>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { replay(transaction, organization_id, &idempotency).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_inference_credential_delivery(
        &self,
        bundle: CreateInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = replay(
                        transaction,
                        bundle.credential.organization_id,
                        &bundle.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    insert_credential(transaction, &bundle.credential).await?;
                    insert_receipt(transaction, &bundle.receipt).await?;
                    store_outbox(transaction, &bundle.event).await?;
                    store_credential_audit(
                        transaction,
                        &bundle.credential,
                        "identity.inference-credential.created",
                        bundle.event.correlation_id,
                        true,
                    )
                    .await?;
                    store_idempotency(
                        transaction,
                        &bundle.idempotency,
                        &reference(&bundle.credential),
                    )
                    .await?;
                    Ok(InferenceCredentialWrite {
                        credential: bundle.credential,
                        receipt: Some(bundle.receipt),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn rotate_inference_credential(
        &self,
        bundle: RotateInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = replay(
                        transaction,
                        bundle.credential.organization_id,
                        &bundle.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let existing = lock_credential(
                        transaction,
                        bundle.credential.organization_id,
                        bundle.credential.id,
                    )
                    .await?;
                    bundle
                        .credential
                        .validate_transition_from(&existing, bundle.expected_aggregate_version)
                        .map_err(RepositoryError::Conflict)?;
                    update_credential_row(
                        transaction,
                        &bundle.credential,
                        bundle.expected_aggregate_version,
                    )
                    .await?;
                    delete_receipt(transaction, bundle.credential.id.as_uuid()).await?;
                    insert_receipt(transaction, &bundle.receipt).await?;
                    store_outbox(transaction, &bundle.event).await?;
                    store_credential_audit(
                        transaction,
                        &bundle.credential,
                        "identity.inference-credential.rotated",
                        bundle.event.correlation_id,
                        true,
                    )
                    .await?;
                    store_idempotency(
                        transaction,
                        &bundle.idempotency,
                        &reference(&bundle.credential),
                    )
                    .await?;
                    Ok(InferenceCredentialWrite {
                        credential: bundle.credential,
                        receipt: Some(bundle.receipt),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_inference_credential(
        &self,
        bundle: RevokeInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = replay(
                        transaction,
                        bundle.credential.organization_id,
                        &bundle.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let existing = lock_credential(
                        transaction,
                        bundle.credential.organization_id,
                        bundle.credential.id,
                    )
                    .await?;
                    let changed = bundle.event.is_some();
                    if changed {
                        bundle
                            .credential
                            .validate_transition_from(&existing, bundle.expected_aggregate_version)
                            .map_err(RepositoryError::Conflict)?;
                        update_credential_row(
                            transaction,
                            &bundle.credential,
                            bundle.expected_aggregate_version,
                        )
                        .await?;
                    } else if existing != bundle.credential
                        || existing.aggregate_version() != bundle.expected_aggregate_version
                    {
                        return Err(RepositoryError::Conflict(
                            "inference credential changed while applying revocation".into(),
                        )
                        .into());
                    }
                    delete_receipt(transaction, bundle.credential.id.as_uuid()).await?;
                    if let Some(event) = &bundle.event {
                        store_outbox(transaction, event).await?;
                    }
                    store_credential_audit(
                        transaction,
                        &bundle.credential,
                        "identity.inference-credential.revoked",
                        bundle.request_id,
                        changed,
                    )
                    .await?;
                    store_idempotency(
                        transaction,
                        &bundle.idempotency,
                        &reference(&bundle.credential),
                    )
                    .await?;
                    Ok(InferenceCredentialWrite {
                        credential: bundle.credential,
                        receipt: None,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn sweep_expired_inference_credential_delivery_receipts(
        &self,
        expired_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        let expired_at = canonical_timestamp(expired_at);
        if limit == 0 || limit > 10_000 {
            return Err(RepositoryError::Conflict(
                "inference credential delivery receipt sweep limit is invalid".into(),
            ));
        }
        let limit = u64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict(
                "inference credential delivery receipt sweep limit is invalid".into(),
            )
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(
                    async move { sweep_expired_receipts(transaction, expired_at, limit).await },
                )
            })
            .await
            .map_err(transaction_error)
    }
}

async fn sweep_expired_receipts(
    transaction: &PostgresTransaction,
    expired_at: DateTime<Utc>,
    limit: u64,
) -> Result<usize, PostgresPersistenceError> {
    let credential_ids = fetch_all::<Uuid, _>(
        transaction,
        select_from::<InferenceCredentialDeliveryReceipts>()
            .select(InferenceCredentialDeliveryReceipts::credential_id())
            .filter(InferenceCredentialDeliveryReceipts::expires_at().lte(expired_at))
            .order_by(
                InferenceCredentialDeliveryReceipts::expires_at(),
                OrderDirection::Asc,
            )
            .order_by(
                InferenceCredentialDeliveryReceipts::credential_id(),
                OrderDirection::Asc,
            )
            .limit(limit)
            .for_update(),
    )
    .await?;
    for credential_id in &credential_ids {
        delete_receipt(transaction, *credential_id).await?;
    }
    Ok(credential_ids.len())
}

async fn replay(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<InferenceCredentialWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<InferenceCredentialWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    if replay.value.credential_id.as_uuid().is_nil() || replay.value.generation == 0 {
        return Err(PostgresPersistenceError::Invariant(
            "stored inference credential idempotency reference is invalid".into(),
        ));
    }
    load_write(transaction, organization_id, replay.value, true)
        .await
        .map(Some)
}

async fn load_write(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    reference: InferenceCredentialWriteReference,
    replayed: bool,
) -> Result<InferenceCredentialWrite, PostgresPersistenceError> {
    let credential = fetch_optional::<InferenceCredentialRow, _>(
        transaction,
        credential_query(organization_id, reference.credential_id),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "inference credential idempotency target is missing".into(),
        )
    })?
    .credential()?;
    let receipt = if credential.generation() == reference.generation {
        fetch_receipt(transaction, &credential).await?
    } else {
        None
    };
    Ok(InferenceCredentialWrite {
        credential,
        receipt,
        replayed,
    })
}

async fn lock_credential(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    credential_id: crate::modules::shared_kernel::domain::InferenceCredentialId,
) -> Result<InferenceCredential, PostgresPersistenceError> {
    fetch_optional::<InferenceCredentialRow, _>(
        transaction,
        credential_query(organization_id, credential_id).for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .credential()
    .map_err(Into::into)
}

async fn fetch_receipt(
    transaction: &PostgresTransaction,
    credential: &InferenceCredential,
) -> Result<Option<InferenceCredentialDeliveryReceipt>, PostgresPersistenceError> {
    let row = fetch_optional::<(u64, String, String, DateTime<Utc>, DateTime<Utc>), _>(
        transaction,
        select_from::<InferenceCredentialDeliveryReceipts>()
            .select((
                InferenceCredentialDeliveryReceipts::generation(),
                InferenceCredentialDeliveryReceipts::key_id(),
                InferenceCredentialDeliveryReceipts::ciphertext(),
                InferenceCredentialDeliveryReceipts::expires_at(),
                InferenceCredentialDeliveryReceipts::created_at(),
            ))
            .filter(
                InferenceCredentialDeliveryReceipts::organization_id()
                    .eq(credential.organization_id.as_uuid()),
            )
            .filter(
                InferenceCredentialDeliveryReceipts::credential_id().eq(credential.id.as_uuid()),
            ),
    )
    .await?;
    row.map(|(generation, key_id, ciphertext, expires_at, created_at)| {
        let encrypted = EncryptedSecretValue::new(key_id, ciphertext)
            .map_err(|error| stored_receipt(&error))?;
        let receipt = InferenceCredentialDeliveryReceipt::new(
            credential.organization_id,
            credential.id,
            generation,
            encrypted,
            expires_at,
            created_at,
        )
        .map_err(|error| stored_receipt(&error))?;
        receipt
            .validate_against(credential)
            .map_err(|error| stored_receipt(&error))?;
        Ok(receipt)
    })
    .transpose()
}

async fn insert_receipt(
    transaction: &PostgresTransaction,
    receipt: &InferenceCredentialDeliveryReceipt,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<InferenceCredentialDeliveryReceipts>()
            .value(
                InferenceCredentialDeliveryReceipts::credential_id(),
                receipt.credential_id.as_uuid(),
            )
            .value(
                InferenceCredentialDeliveryReceipts::organization_id(),
                receipt.organization_id.as_uuid(),
            )
            .value(
                InferenceCredentialDeliveryReceipts::generation(),
                receipt.generation,
            )
            .value(
                InferenceCredentialDeliveryReceipts::key_id(),
                receipt.encrypted_value.key_id(),
            )
            .value(
                InferenceCredentialDeliveryReceipts::ciphertext(),
                receipt.encrypted_value.ciphertext(),
            )
            .value(
                InferenceCredentialDeliveryReceipts::expires_at(),
                receipt.expires_at,
            )
            .value(
                InferenceCredentialDeliveryReceipts::created_at(),
                receipt.created_at,
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("inference credential delivery receipt", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn delete_receipt(
    transaction: &PostgresTransaction,
    credential_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    execute(
        transaction,
        delete_from::<InferenceCredentialDeliveryReceipts>()
            .filter(InferenceCredentialDeliveryReceipts::credential_id().eq(credential_id)),
    )
    .await?;
    Ok(())
}

async fn store_credential_audit(
    transaction: &PostgresTransaction,
    credential: &InferenceCredential,
    action: &'static str,
    request_id: Uuid,
    changed: bool,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: None,
            action,
            aggregate_id: credential.id.as_uuid(),
            occurred_at: credential.updated_at(),
            request_id,
            scope: AuditWrite::resource_scope(
                credential.organization_id.as_uuid(),
                credential.project_id,
                Some(credential.environment_id),
            ),
            details: serde_json::json!({
                "projectId": credential.project_id,
                "environmentId": credential.environment_id,
                "generation": credential.generation(),
                "aggregateVersion": credential.aggregate_version(),
                "expiresAt": credential.expires_at(),
                "changed": changed,
            }),
        },
    )
    .await
}

fn reference(credential: &InferenceCredential) -> InferenceCredentialWriteReference {
    InferenceCredentialWriteReference {
        credential_id: credential.id,
        generation: credential.generation(),
    }
}

fn stored_receipt(error: &str) -> PostgresPersistenceError {
    RepositoryError::Storage(format!(
        "stored inference credential delivery receipt is invalid: {error}"
    ))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::{PostgresDialect, Query};

    #[test]
    fn receipt_lookup_is_scoped_by_tenant_and_credential() {
        let query = select_from::<InferenceCredentialDeliveryReceipts>()
            .select(InferenceCredentialDeliveryReceipts::generation())
            .filter(InferenceCredentialDeliveryReceipts::organization_id().eq(Uuid::now_v7()))
            .filter(InferenceCredentialDeliveryReceipts::credential_id().eq(Uuid::now_v7()))
            .compile(&PostgresDialect)
            .expect("compile");

        assert!(query.sql.contains("\"organization_id\" = $1"));
        assert!(query.sql.contains("\"credential_id\" = $2"));
        assert_eq!(query.parameters.len(), 2);
    }
}
