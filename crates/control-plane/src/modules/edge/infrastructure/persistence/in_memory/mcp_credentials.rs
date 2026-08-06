use super::InMemoryEdgeRepository;
use crate::modules::edge::domain::repositories::{
    validate_mcp_credential_resolution, CreateMcpCredentialWrite,
    IMcpCredentialLifecycleRepository, IMcpCredentialRepository, McpCredentialWrite,
    McpCredentialWriteReference, RevokeMcpCredentialWrite, RotateMcpCredentialWrite,
};
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, McpCredentialId, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
impl IMcpCredentialRepository for InMemoryEdgeRepository {
    async fn create_mcp_credential(
        &self,
        credential: McpCredential,
    ) -> Result<McpCredential, RepositoryError> {
        if credential.generation() != 1
            || credential.aggregate_version() != 1
            || credential.created_at() != credential.updated_at()
            || credential.revoked_at().is_some()
        {
            return Err(RepositoryError::Conflict(
                "new MCP credential is not at its initial generation".into(),
            ));
        }
        let mut state = self.state.write().await;
        if state.mcp_credentials.contains_key(&credential.id)
            || state
                .mcp_credential_prefixes
                .contains_key(credential.prefix())
        {
            return Err(RepositoryError::Conflict(
                "MCP credential identity or lookup prefix is already in use".into(),
            ));
        }
        state
            .mcp_credential_prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state
            .mcp_credentials
            .insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn update_mcp_credential(
        &self,
        credential: McpCredential,
        expected_aggregate_version: u64,
    ) -> Result<McpCredential, RepositoryError> {
        let mut state = self.state.write().await;
        let existing = state
            .mcp_credentials
            .get(&credential.id)
            .filter(|existing| existing.organization_id == credential.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        credential
            .validate_transition_from(&existing, expected_aggregate_version)
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
        state
            .mcp_credential_prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state
            .mcp_credentials
            .insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn find_mcp_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: McpCredentialId,
    ) -> Result<Option<McpCredential>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .mcp_credentials
            .get(&credential_id)
            .filter(|credential| credential.organization_id == organization_id)
            .cloned())
    }

    async fn list_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpCredential>, RepositoryError> {
        let mut credentials = self
            .state
            .read()
            .await
            .mcp_credentials
            .values()
            .filter(|credential| {
                credential.organization_id == organization_id
                    && credential.project_id == project_id
                    && credential.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        credentials.sort_by_key(|credential| (credential.created_at(), credential.id));
        Ok(credentials)
    }

    async fn resolve_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        credential_ids: &[McpCredentialId],
    ) -> Result<Vec<McpCredential>, RepositoryError> {
        validate_mcp_credential_resolution(credential_ids)?;
        let state = self.state.read().await;
        let mut credentials = credential_ids
            .iter()
            .filter_map(|credential_id| state.mcp_credentials.get(credential_id))
            .filter(|credential| {
                credential.organization_id == organization_id
                    && credential.project_id == project_id
                    && credential.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        credentials.sort_by_key(|credential| credential.id);
        Ok(credentials)
    }
}

#[async_trait]
impl IMcpCredentialLifecycleRepository for InMemoryEdgeRepository {
    async fn replay_mcp_credential_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<McpCredentialWrite>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, organization_id, idempotency)
    }

    async fn create_mcp_credential_delivery(
        &self,
        bundle: CreateMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replay) = replay(
            &state,
            bundle.credential.organization_id,
            &bundle.idempotency,
        )? {
            return Ok(replay);
        }
        if state.mcp_credentials.contains_key(&bundle.credential.id)
            || state
                .mcp_credential_prefixes
                .contains_key(bundle.credential.prefix())
        {
            return Err(RepositoryError::Conflict(
                "MCP credential identity or lookup prefix is already in use".into(),
            ));
        }
        state
            .mcp_credential_prefixes
            .insert(bundle.credential.prefix().to_owned(), bundle.credential.id);
        state
            .mcp_credentials
            .insert(bundle.credential.id, bundle.credential.clone());
        state
            .mcp_credential_receipts
            .insert(bundle.credential.id, bundle.receipt.clone());
        remember(
            &mut state,
            bundle.idempotency,
            McpCredentialWriteReference {
                credential_id: bundle.credential.id,
                generation: bundle.credential.generation(),
            },
        );
        state.outbox.push(bundle.event);
        Ok(McpCredentialWrite {
            credential: bundle.credential,
            receipt: Some(bundle.receipt),
            replayed: false,
        })
    }

    async fn rotate_mcp_credential_delivery(
        &self,
        bundle: RotateMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replay) = replay(
            &state,
            bundle.credential.organization_id,
            &bundle.idempotency,
        )? {
            return Ok(replay);
        }
        let existing = state
            .mcp_credentials
            .get(&bundle.credential.id)
            .filter(|existing| existing.organization_id == bundle.credential.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        bundle
            .credential
            .validate_transition_from(&existing, bundle.expected_aggregate_version)
            .map_err(RepositoryError::Conflict)?;
        if state
            .mcp_credential_prefixes
            .get(bundle.credential.prefix())
            .is_some_and(|id| *id != bundle.credential.id)
        {
            return Err(RepositoryError::Conflict(
                "MCP credential lookup prefix is already in use".into(),
            ));
        }
        state.mcp_credential_prefixes.remove(existing.prefix());
        state
            .mcp_credential_prefixes
            .insert(bundle.credential.prefix().to_owned(), bundle.credential.id);
        state
            .mcp_credentials
            .insert(bundle.credential.id, bundle.credential.clone());
        state
            .mcp_credential_receipts
            .insert(bundle.credential.id, bundle.receipt.clone());
        remember(
            &mut state,
            bundle.idempotency,
            McpCredentialWriteReference {
                credential_id: bundle.credential.id,
                generation: bundle.credential.generation(),
            },
        );
        state.outbox.push(bundle.event);
        Ok(McpCredentialWrite {
            credential: bundle.credential,
            receipt: Some(bundle.receipt),
            replayed: false,
        })
    }

    async fn revoke_mcp_credential(
        &self,
        bundle: RevokeMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replay) = replay(
            &state,
            bundle.credential.organization_id,
            &bundle.idempotency,
        )? {
            return Ok(replay);
        }
        let existing = state
            .mcp_credentials
            .get(&bundle.credential.id)
            .filter(|existing| existing.organization_id == bundle.credential.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if bundle.event.is_some() {
            bundle
                .credential
                .validate_transition_from(&existing, bundle.expected_aggregate_version)
                .map_err(RepositoryError::Conflict)?;
        } else if existing != bundle.credential
            || existing.aggregate_version() != bundle.expected_aggregate_version
        {
            return Err(RepositoryError::Conflict(
                "MCP credential changed while applying revocation".into(),
            ));
        }
        state
            .mcp_credentials
            .insert(bundle.credential.id, bundle.credential.clone());
        state.mcp_credential_receipts.remove(&bundle.credential.id);
        remember(
            &mut state,
            bundle.idempotency,
            McpCredentialWriteReference {
                credential_id: bundle.credential.id,
                generation: bundle.credential.generation(),
            },
        );
        if let Some(event) = bundle.event {
            state.outbox.push(event);
        }
        Ok(McpCredentialWrite {
            credential: bundle.credential,
            receipt: None,
            replayed: false,
        })
    }

    async fn sweep_expired_mcp_credential_delivery_receipts(
        &self,
        expired_at: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        let expired_at = canonical_timestamp(expired_at);
        if limit == 0 || limit > 10_000 {
            return Err(RepositoryError::Conflict(
                "MCP credential delivery receipt sweep limit is invalid".into(),
            ));
        }
        let mut state = self.state.write().await;
        let mut expired = state
            .mcp_credential_receipts
            .iter()
            .filter(|(_, receipt)| receipt.expires_at <= expired_at)
            .map(|(credential_id, receipt)| (*credential_id, receipt.expires_at))
            .collect::<Vec<_>>();
        expired.sort_by_key(|(credential_id, expires_at)| (*expires_at, *credential_id));
        expired.truncate(limit);
        for (credential_id, _) in &expired {
            state.mcp_credential_receipts.remove(credential_id);
        }
        Ok(expired.len())
    }
}

fn replay(
    state: &super::State,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<McpCredentialWrite>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((digest, reference)) = state.mcp_credential_idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    let credential = state
        .mcp_credentials
        .get(&reference.credential_id)
        .filter(|credential| credential.organization_id == organization_id)
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("MCP credential idempotency target is missing".into())
        })?;
    let receipt = state
        .mcp_credential_receipts
        .get(&credential.id)
        .filter(|receipt| {
            receipt.organization_id == organization_id
                && receipt.generation == reference.generation
                && credential.generation() == reference.generation
        })
        .cloned();
    Ok(Some(McpCredentialWrite {
        credential,
        receipt,
        replayed: true,
    }))
}

fn remember(
    state: &mut super::State,
    idempotency: IdempotencyRequest,
    reference: McpCredentialWriteReference,
) {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    state
        .mcp_credential_idempotency
        .insert(key, (idempotency.request_digest, reference));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::McpCredentialDeliveryReceipt;
    use crate::modules::secrets::domain::EncryptedSecretValue;
    use chrono::{Duration, TimeZone, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const ROTATED_VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    fn credential(
        id: McpCredentialId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        prefix: &str,
    ) -> McpCredential {
        McpCredential::issue(
            id,
            organization_id,
            project_id,
            environment_id,
            prefix,
            VERIFIER,
            now() + Duration::days(30),
            now(),
        )
        .expect("credential")
    }

    #[tokio::test]
    async fn persists_rotation_revocation_and_tenant_non_disclosure() {
        let repository = InMemoryEdgeRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let mut stored = repository
            .create_mcp_credential(credential(
                McpCredentialId::new(),
                organization_id,
                project_id,
                environment_id,
                "a3s_mcp_abc12345def67890",
            ))
            .await
            .expect("create");
        assert_eq!(
            repository
                .find_mcp_credential(organization_id, stored.id)
                .await
                .expect("find"),
            Some(stored.clone())
        );
        assert_eq!(
            repository
                .find_mcp_credential(OrganizationId::new(), stored.id)
                .await
                .expect("tenant non-disclosure"),
            None
        );

        stored
            .rotate(
                "a3s_mcp_def67890abc12345",
                ROTATED_VERIFIER,
                now() + Duration::days(60),
                now() + Duration::minutes(1),
            )
            .expect("rotate");
        stored = repository
            .update_mcp_credential(stored, 1)
            .await
            .expect("persist rotation");
        assert_eq!(stored.generation(), 2);
        assert!(stored.revoke(now() + Duration::minutes(2)).expect("revoke"));
        stored = repository
            .update_mcp_credential(stored, 2)
            .await
            .expect("persist revocation");
        assert_eq!(stored.aggregate_version(), 3);
        assert_eq!(
            repository
                .list_mcp_credentials(organization_id, project_id, environment_id)
                .await
                .expect("list"),
            vec![stored]
        );
    }

    #[tokio::test]
    async fn resolves_only_exact_unique_environment_credentials() {
        let repository = InMemoryEdgeRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let requested = credential(
            McpCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_mcp_abc12345def67890",
        );
        let other = credential(
            McpCredentialId::new(),
            organization_id,
            project_id,
            EnvironmentId::new(),
            "a3s_mcp_def67890abc12345",
        );
        repository
            .create_mcp_credential(requested.clone())
            .await
            .expect("requested");
        repository
            .create_mcp_credential(other.clone())
            .await
            .expect("other environment");

        assert_eq!(
            repository
                .resolve_mcp_credentials(
                    organization_id,
                    project_id,
                    environment_id,
                    &[McpCredentialId::new(), requested.id],
                )
                .await
                .expect("resolve"),
            vec![requested.clone()]
        );
        assert!(matches!(
            repository
                .resolve_mcp_credentials(
                    organization_id,
                    project_id,
                    environment_id,
                    &[requested.id, requested.id],
                )
                .await,
            Err(RepositoryError::Conflict(_))
        ));
        assert!(repository
            .resolve_mcp_credentials(organization_id, project_id, environment_id, &[other.id],)
            .await
            .expect("cross-environment non-disclosure")
            .is_empty());
    }

    #[tokio::test]
    async fn rejects_prefix_collisions_and_stale_updates() {
        let repository = InMemoryEdgeRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let first = repository
            .create_mcp_credential(credential(
                McpCredentialId::new(),
                organization_id,
                project_id,
                environment_id,
                "a3s_mcp_abc12345def67890",
            ))
            .await
            .expect("first");
        assert!(matches!(
            repository
                .create_mcp_credential(credential(
                    McpCredentialId::new(),
                    organization_id,
                    project_id,
                    environment_id,
                    "a3s_mcp_abc12345def67890",
                ))
                .await,
            Err(RepositoryError::Conflict(_))
        ));

        let mut rotated = first.clone();
        rotated
            .rotate(
                "a3s_mcp_def67890abc12345",
                ROTATED_VERIFIER,
                now() + Duration::days(60),
                now() + Duration::minutes(1),
            )
            .expect("rotate");
        repository
            .update_mcp_credential(rotated.clone(), 1)
            .await
            .expect("update");
        assert!(matches!(
            repository.update_mcp_credential(rotated, 1).await,
            Err(RepositoryError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn sweeps_expired_delivery_receipts_in_bounded_expiry_order() {
        let repository = InMemoryEdgeRepository::new();
        let organization_id = OrganizationId::new();
        let first_id = McpCredentialId::new();
        let second_id = McpCredentialId::new();
        let active_id = McpCredentialId::new();
        let receipt = |credential_id, expires_at| {
            McpCredentialDeliveryReceipt::new(
                organization_id,
                credential_id,
                1,
                EncryptedSecretValue::new("test-key", "encrypted-value").expect("encrypted value"),
                expires_at,
                now() - Duration::minutes(20),
            )
            .expect("delivery receipt")
        };
        {
            let mut state = repository.state.write().await;
            state
                .mcp_credential_receipts
                .insert(first_id, receipt(first_id, now() - Duration::minutes(10)));
            state
                .mcp_credential_receipts
                .insert(second_id, receipt(second_id, now() - Duration::minutes(5)));
            state
                .mcp_credential_receipts
                .insert(active_id, receipt(active_id, now() + Duration::minutes(5)));
        }

        assert_eq!(
            repository
                .sweep_expired_mcp_credential_delivery_receipts(now(), 1)
                .await
                .expect("first sweep"),
            1
        );
        {
            let state = repository.state.read().await;
            assert!(!state.mcp_credential_receipts.contains_key(&first_id));
            assert!(state.mcp_credential_receipts.contains_key(&second_id));
            assert!(state.mcp_credential_receipts.contains_key(&active_id));
        }
        assert_eq!(
            repository
                .sweep_expired_mcp_credential_delivery_receipts(now(), 100)
                .await
                .expect("second sweep"),
            1
        );
        let state = repository.state.read().await;
        assert!(!state.mcp_credential_receipts.contains_key(&second_id));
        assert!(state.mcp_credential_receipts.contains_key(&active_id));
    }
}
