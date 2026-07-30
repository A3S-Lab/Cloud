use super::{InMemoryEdgeRepository, State};
use crate::modules::edge::domain::repositories::{
    IMcpCredentialLifecycleRepository, McpCredentialLifecycleReference,
    McpCredentialLifecycleResult, StoreMcpCredentialLifecycle,
    MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH, MCP_CREDENTIAL_IDENTITY_CONFLICT,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId,
    RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl IMcpCredentialLifecycleRepository for InMemoryEdgeRepository {
    async fn replay_mcp_credential_lifecycle(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        idempotency: &IdempotencyRequest,
        observed_at: DateTime<Utc>,
    ) -> Result<Option<McpCredentialLifecycleResult>, RepositoryError> {
        replay(
            &*self.state.read().await,
            organization_id,
            project_id,
            environment_id,
            idempotency,
            observed_at,
        )
    }

    async fn store_mcp_credential_lifecycle(
        &self,
        bundle: StoreMcpCredentialLifecycle,
    ) -> Result<McpCredentialLifecycleResult, RepositoryError> {
        bundle.validate()?;
        let mut state = self.state.write().await;
        if let Some(replay) = replay(
            &state,
            bundle.credential.organization_id,
            bundle.credential.project_id,
            bundle.credential.environment_id,
            &bundle.idempotency,
            bundle.observed_at,
        )? {
            return Ok(replay);
        }

        let credential = bundle.credential.clone();
        match bundle.expected_aggregate_version {
            None => {
                if state.mcp_credentials.contains_key(&credential.id)
                    || state
                        .mcp_credential_prefixes
                        .contains_key(credential.prefix())
                {
                    return Err(RepositoryError::Conflict(
                        MCP_CREDENTIAL_IDENTITY_CONFLICT.into(),
                    ));
                }
            }
            Some(expected_version) => {
                let existing = state
                    .mcp_credentials
                    .get(&credential.id)
                    .filter(|existing| {
                        existing.organization_id == credential.organization_id
                            && existing.project_id == credential.project_id
                            && existing.environment_id == credential.environment_id
                    })
                    .cloned()
                    .ok_or(RepositoryError::NotFound)?;
                credential
                    .validate_transition_from(&existing, expected_version)
                    .map_err(RepositoryError::Conflict)?;
                if state
                    .mcp_credential_prefixes
                    .get(credential.prefix())
                    .is_some_and(|id| *id != credential.id)
                {
                    return Err(RepositoryError::Conflict(
                        "MCP credential lookup prefix is already in use".into(),
                    ));
                }
                state.mcp_credential_prefixes.remove(existing.prefix());
            }
        }

        state
            .mcp_credential_prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state
            .mcp_credentials
            .insert(credential.id, credential.clone());
        if let Some(delivery) = bundle.delivery.clone() {
            state
                .mcp_credential_deliveries
                .insert(credential.id, delivery);
        } else {
            state.mcp_credential_deliveries.remove(&credential.id);
        }
        let reference = McpCredentialLifecycleReference::from_bundle(&bundle);
        state.mcp_credential_lifecycle_idempotency.insert(
            (
                bundle.idempotency.scope.clone(),
                bundle.idempotency.key.clone(),
            ),
            (bundle.idempotency.request_digest, reference),
        );
        state.outbox.push(bundle.event);
        Ok(McpCredentialLifecycleResult {
            credential,
            delivery: bundle.delivery,
            replayed: false,
        })
    }

    async fn purge_expired_mcp_credential_deliveries(
        &self,
        observed_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        if limit == 0 || limit > MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH {
            return Err(RepositoryError::Conflict(
                "MCP credential delivery purge limit must be between 1 and 10000".into(),
            ));
        }
        let observed_at = canonical_timestamp(observed_at);
        let mut state = self.state.write().await;
        let expired = state
            .mcp_credential_deliveries
            .values()
            .filter(|delivery| !delivery.is_available_at(observed_at))
            .map(|delivery| {
                (
                    delivery.expires_at(),
                    delivery.credential_id(),
                    delivery.generation(),
                )
            })
            .collect::<Vec<_>>();
        let mut expired = expired;
        expired.sort_unstable();
        expired.truncate(limit);
        let mut purged = 0;
        for (_, credential_id, generation) in expired {
            if state
                .mcp_credential_deliveries
                .get(&credential_id)
                .is_some_and(|delivery| {
                    delivery.generation() == generation && !delivery.is_available_at(observed_at)
                })
            {
                state.mcp_credential_deliveries.remove(&credential_id);
                purged += 1;
            }
        }
        Ok(purged)
    }
}

fn replay(
    state: &State,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    idempotency: &IdempotencyRequest,
    observed_at: DateTime<Utc>,
) -> Result<Option<McpCredentialLifecycleResult>, RepositoryError> {
    let Some((digest, reference)) = state
        .mcp_credential_lifecycle_idempotency
        .get(&(idempotency.scope.clone(), idempotency.key.clone()))
    else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    if reference.organization_id != organization_id
        || reference.project_id != project_id
        || reference.environment_id != environment_id
    {
        return Err(RepositoryError::NotFound);
    }
    let credential = state
        .mcp_credentials
        .get(&reference.credential_id)
        .filter(|credential| reference.matches_credential(credential))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "MCP credential lifecycle advanced beyond this idempotent request".into(),
            )
        })?;
    let delivery = if reference.has_delivery {
        Some(
            state
                .mcp_credential_deliveries
                .get(&reference.credential_id)
                .filter(|delivery| {
                    delivery.matches_credential(&credential)
                        && delivery.is_available_at(canonical_timestamp(observed_at))
                })
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Conflict(
                        "MCP credential recovery window is no longer available".into(),
                    )
                })?,
        )
    } else {
        None
    };
    Ok(Some(McpCredentialLifecycleResult {
        credential,
        delivery,
        replayed: true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::events::McpCredentialChanged;
    use crate::modules::edge::domain::{McpCredential, McpCredentialDelivery};
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
    };
    use chrono::{Duration, TimeZone};
    use uuid::Uuid;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const ROTATED_VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    fn delivery(credential: &McpCredential, ciphertext: &str) -> McpCredentialDelivery {
        McpCredentialDelivery::new(
            credential.organization_id,
            credential.project_id,
            credential.environment_id,
            credential.id,
            credential.generation(),
            "local:v1",
            ciphertext,
            credential.updated_at(),
            credential.updated_at() + Duration::minutes(10),
        )
        .expect("delivery")
    }

    fn idempotency(scope: &str, key: &str, request: &str) -> IdempotencyRequest {
        IdempotencyRequest::new(scope, key, request.as_bytes()).expect("idempotency")
    }

    fn bundle(
        credential: McpCredential,
        expected_aggregate_version: Option<u64>,
        delivery: Option<McpCredentialDelivery>,
        idempotency: IdempotencyRequest,
        observed_at: DateTime<Utc>,
    ) -> StoreMcpCredentialLifecycle {
        let event =
            McpCredentialChanged::envelope(&credential, Uuid::new_v4()).expect("event envelope");
        StoreMcpCredentialLifecycle {
            credential,
            expected_aggregate_version,
            delivery,
            observed_at,
            idempotency,
            event,
        }
    }

    #[tokio::test]
    async fn replays_only_the_exact_current_secret_window_and_never_remints_old_material() {
        let repository = InMemoryEdgeRepository::new();
        let issued = McpCredential::issue(
            McpCredentialId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now() + Duration::days(30),
            now(),
        )
        .expect("credential");
        let issue_key = idempotency("mcp-credentials.issue", "issue-key", "same issuance");
        let first = repository
            .store_mcp_credential_lifecycle(bundle(
                issued.clone(),
                None,
                Some(delivery(&issued, "issue-ciphertext")),
                issue_key.clone(),
                now(),
            ))
            .await
            .expect("store issuance");
        assert!(!first.replayed);
        assert_eq!(
            first.delivery.as_ref().map(|value| value.ciphertext()),
            Some("issue-ciphertext")
        );
        let replay = repository
            .replay_mcp_credential_lifecycle(
                issued.organization_id,
                issued.project_id,
                issued.environment_id,
                &issue_key,
                now() + Duration::minutes(1),
            )
            .await
            .expect("replay issuance")
            .expect("stored replay");
        assert!(replay.replayed);
        assert_eq!(
            replay,
            McpCredentialLifecycleResult {
                replayed: true,
                ..first.clone()
            }
        );
        assert_eq!(
            repository
                .replay_mcp_credential_lifecycle(
                    OrganizationId::new(),
                    issued.project_id,
                    issued.environment_id,
                    &issue_key,
                    now() + Duration::minutes(1),
                )
                .await,
            Err(RepositoryError::NotFound)
        );
        assert!(matches!(
            repository
                .replay_mcp_credential_lifecycle(
                    issued.organization_id,
                    issued.project_id,
                    issued.environment_id,
                    &idempotency("mcp-credentials.issue", "issue-key", "different issuance"),
                    now(),
                )
                .await,
            Err(RepositoryError::IdempotencyConflict)
        ));

        let mut rotated = issued.clone();
        rotated
            .rotate(
                "a3s_mcp_def67890abc12345",
                ROTATED_VERIFIER,
                now() + Duration::days(60),
                now() + Duration::minutes(2),
            )
            .expect("rotate");
        let rotate_key = idempotency("mcp-credentials.rotate", "rotate-key", "same rotation");
        repository
            .store_mcp_credential_lifecycle(bundle(
                rotated.clone(),
                Some(issued.aggregate_version()),
                Some(delivery(&rotated, "rotated-ciphertext")),
                rotate_key.clone(),
                now() + Duration::minutes(2),
            ))
            .await
            .expect("store rotation");
        assert!(matches!(
            repository
                .replay_mcp_credential_lifecycle(
                    issued.organization_id,
                    issued.project_id,
                    issued.environment_id,
                    &issue_key,
                    now() + Duration::minutes(3),
                )
                .await,
            Err(RepositoryError::Conflict(_))
        ));
        assert_eq!(
            repository
                .purge_expired_mcp_credential_deliveries(
                    now() + Duration::minutes(12),
                    MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH,
                )
                .await
                .expect("purge"),
            1
        );
        assert!(matches!(
            repository
                .replay_mcp_credential_lifecycle(
                    rotated.organization_id,
                    rotated.project_id,
                    rotated.environment_id,
                    &rotate_key,
                    now() + Duration::minutes(12),
                )
                .await,
            Err(RepositoryError::Conflict(_))
        ));
        assert_eq!(
            repository
                .purge_expired_mcp_credential_deliveries(now(), 0)
                .await,
            Err(RepositoryError::Conflict(
                "MCP credential delivery purge limit must be between 1 and 10000".into()
            ))
        );
    }

    #[tokio::test]
    async fn revocation_atomically_removes_delivery_and_replays_without_secret_material() {
        let repository = InMemoryEdgeRepository::new();
        let mut credential = McpCredential::issue(
            McpCredentialId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now() + Duration::days(30),
            now(),
        )
        .expect("credential");
        repository
            .store_mcp_credential_lifecycle(bundle(
                credential.clone(),
                None,
                Some(delivery(&credential, "issue-ciphertext")),
                idempotency("mcp-credentials.issue", "issue-key", "issuance"),
                now(),
            ))
            .await
            .expect("issue");

        let expected_version = credential.aggregate_version();
        credential
            .revoke(now() + Duration::minutes(1))
            .expect("revoke");
        let revoke_key = idempotency("mcp-credentials.revoke", "revoke-key", "revocation");
        let revoked = repository
            .store_mcp_credential_lifecycle(bundle(
                credential.clone(),
                Some(expected_version),
                None,
                revoke_key.clone(),
                now() + Duration::minutes(1),
            ))
            .await
            .expect("persist revocation");
        assert!(revoked.delivery.is_none());
        let replay = repository
            .replay_mcp_credential_lifecycle(
                credential.organization_id,
                credential.project_id,
                credential.environment_id,
                &revoke_key,
                now() + Duration::days(1),
            )
            .await
            .expect("replay revocation")
            .expect("stored revocation");
        assert!(replay.replayed);
        assert_eq!(replay.credential, credential);
        assert!(replay.delivery.is_none());
        assert_eq!(
            repository
                .purge_expired_mcp_credential_deliveries(now() + Duration::days(1), 100)
                .await
                .expect("nothing to purge"),
            0
        );
        assert_eq!(repository.outbox_events().await.len(), 2);
    }
}
