use super::postgres::PostgresEdgeRepository;
use super::postgres_mcp_credentials::{
    credential_query, insert_credential, update_credential_row, McpCredentialRow,
};
use super::postgres_schema::McpCredentialDeliveryReceipts;
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, require_one_row,
    store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    CreateMcpCredentialWrite, IMcpCredentialLifecycleRepository, McpCredentialWrite,
    McpCredentialWriteReference, RevokeMcpCredentialWrite, RotateMcpCredentialWrite,
};
use crate::modules::edge::domain::{McpCredential, McpCredentialDeliveryReceipt};
use crate::modules::secrets::domain::EncryptedSecretValue;
use crate::modules::shared_kernel::domain::{IdempotencyRequest, OrganizationId, RepositoryError};
use a3s_orm::{delete_from, insert_into, select_from, Expression, PostgresTransaction};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[async_trait]
impl IMcpCredentialLifecycleRepository for PostgresEdgeRepository {
    async fn replay_mcp_credential_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<McpCredentialWrite>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { replay(transaction, organization_id, &idempotency).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_mcp_credential_delivery(
        &self,
        bundle: CreateMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = replay(
                        transaction,
                        bundle.credential.organization_id,
                        &bundle.idempotency,
                    )
                    .await?
                    {
                        return Ok(replay);
                    }
                    insert_credential(transaction, &bundle.credential).await?;
                    insert_receipt(transaction, &bundle.receipt).await?;
                    store_outbox(transaction, &bundle.event).await?;
                    store_credential_audit(
                        transaction,
                        &bundle.credential,
                        "edge.mcp-credential.created",
                        bundle.event.correlation_id,
                        true,
                    )
                    .await?;
                    let reference = reference(&bundle.credential);
                    store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                    Ok(McpCredentialWrite {
                        credential: bundle.credential,
                        receipt: Some(bundle.receipt),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn rotate_mcp_credential_delivery(
        &self,
        bundle: RotateMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = replay(
                        transaction,
                        bundle.credential.organization_id,
                        &bundle.idempotency,
                    )
                    .await?
                    {
                        return Ok(replay);
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
                    replace_receipt(transaction, &bundle.receipt).await?;
                    store_outbox(transaction, &bundle.event).await?;
                    store_credential_audit(
                        transaction,
                        &bundle.credential,
                        "edge.mcp-credential.rotated",
                        bundle.event.correlation_id,
                        true,
                    )
                    .await?;
                    let reference = reference(&bundle.credential);
                    store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                    Ok(McpCredentialWrite {
                        credential: bundle.credential,
                        receipt: Some(bundle.receipt),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_mcp_credential(
        &self,
        bundle: RevokeMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = replay(
                        transaction,
                        bundle.credential.organization_id,
                        &bundle.idempotency,
                    )
                    .await?
                    {
                        return Ok(replay);
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
                            "MCP credential changed while applying revocation".into(),
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
                        "edge.mcp-credential.revoked",
                        bundle.request_id,
                        changed,
                    )
                    .await?;
                    let reference = reference(&bundle.credential);
                    store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                    Ok(McpCredentialWrite {
                        credential: bundle.credential,
                        receipt: None,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn replay(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<McpCredentialWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<McpCredentialWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    if replay.value.credential_id.as_uuid().is_nil() || replay.value.generation == 0 {
        return Err(PostgresPersistenceError::Invariant(
            "stored MCP credential idempotency reference is invalid".into(),
        ));
    }
    load_write(transaction, organization_id, replay.value, true)
        .await
        .map(Some)
}

async fn load_write(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    reference: McpCredentialWriteReference,
    replayed: bool,
) -> Result<McpCredentialWrite, PostgresPersistenceError> {
    let credential = fetch_optional::<McpCredentialRow, _>(
        transaction,
        credential_query(organization_id, reference.credential_id),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("MCP credential idempotency target is missing".into())
    })?
    .credential()?;
    let receipt = if credential.generation() == reference.generation {
        fetch_receipt(transaction, &credential).await?
    } else {
        None
    };
    Ok(McpCredentialWrite {
        credential,
        receipt,
        replayed,
    })
}

async fn lock_credential(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    credential_id: crate::modules::shared_kernel::domain::McpCredentialId,
) -> Result<McpCredential, PostgresPersistenceError> {
    fetch_optional::<McpCredentialRow, _>(
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
    credential: &McpCredential,
) -> Result<Option<McpCredentialDeliveryReceipt>, PostgresPersistenceError> {
    let row = fetch_optional::<(u64, String, String, DateTime<Utc>, DateTime<Utc>), _>(
        transaction,
        select_from::<McpCredentialDeliveryReceipts>()
            .select((
                McpCredentialDeliveryReceipts::generation(),
                McpCredentialDeliveryReceipts::key_id(),
                McpCredentialDeliveryReceipts::ciphertext(),
                McpCredentialDeliveryReceipts::expires_at(),
                McpCredentialDeliveryReceipts::created_at(),
            ))
            .filter(
                McpCredentialDeliveryReceipts::organization_id()
                    .eq(credential.organization_id.as_uuid()),
            )
            .filter(McpCredentialDeliveryReceipts::credential_id().eq(credential.id.as_uuid())),
    )
    .await?;
    row.map(|(generation, key_id, ciphertext, expires_at, created_at)| {
        let encrypted = EncryptedSecretValue::new(key_id, ciphertext)
            .map_err(|error| stored_receipt(&error))?;
        let receipt = McpCredentialDeliveryReceipt::new(
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
    receipt: &McpCredentialDeliveryReceipt,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<McpCredentialDeliveryReceipts>()
            .value(
                McpCredentialDeliveryReceipts::credential_id(),
                receipt.credential_id.as_uuid(),
            )
            .value(
                McpCredentialDeliveryReceipts::organization_id(),
                receipt.organization_id.as_uuid(),
            )
            .value(
                McpCredentialDeliveryReceipts::generation(),
                receipt.generation,
            )
            .value(
                McpCredentialDeliveryReceipts::key_id(),
                receipt.encrypted_value.key_id(),
            )
            .value(
                McpCredentialDeliveryReceipts::ciphertext(),
                receipt.encrypted_value.ciphertext(),
            )
            .value(
                McpCredentialDeliveryReceipts::expires_at(),
                receipt.expires_at,
            )
            .value(
                McpCredentialDeliveryReceipts::created_at(),
                receipt.created_at,
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("MCP credential delivery receipt", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn replace_receipt(
    transaction: &PostgresTransaction,
    receipt: &McpCredentialDeliveryReceipt,
) -> Result<(), PostgresPersistenceError> {
    delete_receipt(transaction, receipt.credential_id.as_uuid()).await?;
    insert_receipt(transaction, receipt).await
}

async fn delete_receipt(
    transaction: &PostgresTransaction,
    credential_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    execute(
        transaction,
        delete_from::<McpCredentialDeliveryReceipts>()
            .filter(McpCredentialDeliveryReceipts::credential_id().eq(credential_id)),
    )
    .await?;
    Ok(())
}

async fn store_credential_audit(
    transaction: &PostgresTransaction,
    credential: &McpCredential,
    action: &'static str,
    request_id: Uuid,
    changed: bool,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: credential.organization_id.as_uuid(),
            actor_id: None,
            action,
            aggregate_id: credential.id.as_uuid(),
            occurred_at: credential.updated_at(),
            request_id,
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

fn reference(credential: &McpCredential) -> McpCredentialWriteReference {
    McpCredentialWriteReference {
        credential_id: credential.id,
        generation: credential.generation(),
    }
}

fn stored_receipt(error: &str) -> PostgresPersistenceError {
    RepositoryError::Storage(format!(
        "stored MCP credential delivery receipt is invalid: {error}"
    ))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::{PostgresDialect, Query};

    #[test]
    fn receipt_lookup_is_scoped_by_tenant_and_credential() {
        let query = select_from::<McpCredentialDeliveryReceipts>()
            .select(McpCredentialDeliveryReceipts::generation())
            .filter(McpCredentialDeliveryReceipts::organization_id().eq(Uuid::now_v7()))
            .filter(McpCredentialDeliveryReceipts::credential_id().eq(Uuid::now_v7()))
            .compile(&PostgresDialect)
            .expect("compile");

        assert!(query.sql.contains("\"organization_id\" = $1"));
        assert!(query.sql.contains("\"credential_id\" = $2"));
        assert_eq!(query.parameters.len(), 2);
    }
}
