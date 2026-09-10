use crate::modules::identity::domain::entities::{
    InferenceCredential, InferenceCredentialDeliveryReceipt,
};
use crate::modules::identity::domain::repositories::{
    CreateInferenceCredentialWrite, IInferenceCredentialLifecycleRepository,
    IInferenceCredentialRepository, InferenceCredentialWrite, RotateInferenceCredentialWrite,
    RevokeInferenceCredentialWrite,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, InferenceCredentialId, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct WriteReference {
    credential_id: InferenceCredentialId,
    generation: u64,
}

#[derive(Default)]
struct State {
    credentials: HashMap<InferenceCredentialId, InferenceCredential>,
    prefixes: HashMap<String, InferenceCredentialId>,
    receipts: HashMap<InferenceCredentialId, InferenceCredentialDeliveryReceipt>,
    idempotency: HashMap<(String, String), (String, WriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

/// Standalone in-memory authority for Identity inference credentials.
#[derive(Clone, Default)]
pub struct InMemoryInferenceCredentialRepository {
    state: Arc<RwLock<State>>,
}

#[async_trait]
impl IInferenceCredentialRepository for InMemoryInferenceCredentialRepository {
    async fn create_inference_credential(
        &self,
        credential: InferenceCredential,
    ) -> Result<InferenceCredential, RepositoryError> {
        if credential.generation() != 1
            || credential.aggregate_version() != 1
            || credential.created_at() != credential.updated_at()
            || credential.revoked_at().is_some()
        {
            return Err(RepositoryError::Conflict(
                "new inference credential is not at its initial generation".into(),
            ));
        }
        let mut state = self.state.write().await;
        if state.credentials.contains_key(&credential.id)
            || state.prefixes.contains_key(credential.prefix())
        {
            return Err(RepositoryError::Conflict(
                "inference credential identity or lookup prefix is already in use".into(),
            ));
        }
        state
            .prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state.credentials.insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn update_inference_credential(
        &self,
        credential: InferenceCredential,
        expected_aggregate_version: u64,
    ) -> Result<InferenceCredential, RepositoryError> {
        let mut state = self.state.write().await;
        let existing = state
            .credentials
            .get(&credential.id)
            .filter(|existing| existing.organization_id == credential.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        credential
            .validate_transition_from(&existing, expected_aggregate_version)
            .map_err(RepositoryError::Conflict)?;
        if state
            .prefixes
            .get(credential.prefix())
            .is_some_and(|id| *id != credential.id)
        {
            return Err(RepositoryError::Conflict(
                "inference credential lookup prefix is already in use".into(),
            ));
        }
        state.prefixes.remove(existing.prefix());
        state
            .prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state.credentials.insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn find_inference_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: InferenceCredentialId,
    ) -> Result<Option<InferenceCredential>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .credentials
            .get(&credential_id)
            .filter(|credential| credential.organization_id == organization_id)
            .cloned())
    }

    async fn list_inference_credentials_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceCredential>, RepositoryError> {
        let state = self.state.read().await;
        let mut credentials = state
            .credentials
            .values()
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
impl IInferenceCredentialLifecycleRepository for InMemoryInferenceCredentialRepository {
    async fn replay_inference_credential_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<InferenceCredentialWrite>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, organization_id, idempotency)
    }

    async fn create_inference_credential_delivery(
        &self,
        bundle: CreateInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(
            &state,
            bundle.credential.organization_id,
            &bundle.idempotency,
        )? {
            return Ok(replayed);
        }
        if state.credentials.contains_key(&bundle.credential.id)
            || state.prefixes.contains_key(bundle.credential.prefix())
        {
            return Err(RepositoryError::Conflict(
                "inference credential identity or lookup prefix is already in use".into(),
            ));
        }
        state
            .prefixes
            .insert(bundle.credential.prefix().to_owned(), bundle.credential.id);
        state
            .credentials
            .insert(bundle.credential.id, bundle.credential.clone());
        state
            .receipts
            .insert(bundle.credential.id, bundle.receipt.clone());
        remember(
            &mut state,
            bundle.idempotency,
            WriteReference {
                credential_id: bundle.credential.id,
                generation: bundle.credential.generation(),
            },
        );
        state.outbox.push(bundle.event);
        Ok(InferenceCredentialWrite {
            credential: bundle.credential,
            receipt: Some(bundle.receipt),
            replayed: false,
        })
    }

    async fn rotate_inference_credential(
        &self,
        bundle: RotateInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(
            &state,
            bundle.credential.organization_id,
            &bundle.idempotency,
        )? {
            return Ok(replayed);
        }
        let existing = state
            .credentials
            .get(&bundle.credential.id)
            .filter(|existing| existing.organization_id == bundle.credential.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        bundle
            .credential
            .validate_transition_from(&existing, bundle.expected_aggregate_version)
            .map_err(RepositoryError::Conflict)?;
        if state
            .prefixes
            .get(bundle.credential.prefix())
            .is_some_and(|id| *id != bundle.credential.id)
        {
            return Err(RepositoryError::Conflict(
                "inference credential lookup prefix is already in use".into(),
            ));
        }
        state.prefixes.remove(existing.prefix());
        state
            .prefixes
            .insert(bundle.credential.prefix().to_owned(), bundle.credential.id);
        state
            .credentials
            .insert(bundle.credential.id, bundle.credential.clone());
        state
            .receipts
            .insert(bundle.credential.id, bundle.receipt.clone());
        remember(
            &mut state,
            bundle.idempotency,
            WriteReference {
                credential_id: bundle.credential.id,
                generation: bundle.credential.generation(),
            },
        );
        state.outbox.push(bundle.event);
        Ok(InferenceCredentialWrite {
            credential: bundle.credential,
            receipt: Some(bundle.receipt),
            replayed: false,
        })
    }

    async fn revoke_inference_credential(
        &self,
        bundle: RevokeInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(
            &state,
            bundle.credential.organization_id,
            &bundle.idempotency,
        )? {
            return Ok(replayed);
        }
        let existing = state
            .credentials
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
                "inference credential changed while applying revocation".into(),
            ));
        }
        state
            .credentials
            .insert(bundle.credential.id, bundle.credential.clone());
        state.receipts.remove(&bundle.credential.id);
        remember(
            &mut state,
            bundle.idempotency,
            WriteReference {
                credential_id: bundle.credential.id,
                generation: bundle.credential.generation(),
            },
        );
        if let Some(event) = bundle.event {
            state.outbox.push(event);
        }
        Ok(InferenceCredentialWrite {
            credential: bundle.credential,
            receipt: None,
            replayed: false,
        })
    }

    async fn sweep_expired_inference_credential_delivery_receipts(
        &self,
        expired_at: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        let expired_at = canonical_timestamp(expired_at);
        if limit == 0 || limit > 10_000 {
            return Err(RepositoryError::Conflict(
                "inference credential delivery receipt sweep limit is invalid".into(),
            ));
        }
        let mut state = self.state.write().await;
        let mut expired = state
            .receipts
            .iter()
            .filter(|(_, receipt)| receipt.expires_at <= expired_at)
            .map(|(credential_id, receipt)| (*credential_id, receipt.expires_at))
            .collect::<Vec<_>>();
        expired.sort_by_key(|(credential_id, expires_at)| (*expires_at, *credential_id));
        expired.truncate(limit);
        for (credential_id, _) in &expired {
            state.receipts.remove(credential_id);
        }
        Ok(expired.len())
    }
}

fn replay(
    state: &State,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<InferenceCredentialWrite>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((digest, reference)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::Conflict(
            "inference credential idempotency key was reused with a different request".into(),
        ));
    }
    let credential = state
        .credentials
        .get(&reference.credential_id)
        .filter(|credential| credential.organization_id == organization_id)
        .cloned()
        .ok_or(RepositoryError::NotFound)?;
    if credential.generation() != reference.generation {
        return Err(RepositoryError::Conflict(
            "inference credential generation changed after the remembered write".into(),
        ));
    }
    Ok(Some(InferenceCredentialWrite {
        receipt: state.receipts.get(&credential.id).cloned(),
        credential,
        replayed: true,
    }))
}

fn remember(state: &mut State, idempotency: IdempotencyRequest, reference: WriteReference) {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    state
        .idempotency
        .insert(key, (idempotency.request_digest, reference));
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{render_inference_policy_acl, require_inference_tokenizer_revision};
    use chrono::{Duration, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[tokio::test]
    async fn issue_store_project_and_revoke_round_trip() {
        let repo = InMemoryInferenceCredentialRepository::default();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let created_at = Utc::now();
        let credential = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(2),
            created_at,
        )
        .unwrap();
        let stored = repo
            .create_inference_credential(credential.clone())
            .await
            .unwrap();
        let listed = repo
            .list_inference_credentials_by_environment(organization_id, project_id, environment_id)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        let projection = listed[0].gateway_projection().unwrap();
        let acl = render_inference_policy_acl(stored.expires_at(), &[projection]).unwrap();
        require_inference_tokenizer_revision(&acl).unwrap();
        assert!(acl.contains("a3s_inf_0123456789abcdef"));
        assert!(acl.contains("revoked = false"));

        let mut revoked = listed[0].clone();
        let version = revoked.aggregate_version();
        revoked
            .revoke(revoked.updated_at() + Duration::seconds(1))
            .unwrap();
        let revoked = repo
            .update_inference_credential(revoked, version)
            .await
            .unwrap();
        assert!(revoked.gateway_projection().unwrap().revoked);
    }

    #[tokio::test]
    async fn rejects_duplicate_prefix() {
        let repo = InMemoryInferenceCredentialRepository::default();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let created_at = Utc::now();
        let first = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(1),
            created_at,
        )
        .unwrap();
        repo.create_inference_credential(first).await.unwrap();
        let second = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(1),
            created_at,
        )
        .unwrap();
        let err = repo.create_inference_credential(second).await.unwrap_err();
        assert!(matches!(err, RepositoryError::Conflict(_)));
    }

    #[tokio::test]
    async fn sweeps_expired_delivery_receipts_in_bounded_expiry_order() {
        use crate::modules::secrets::domain::EncryptedSecretValue;

        let repository = InMemoryInferenceCredentialRepository::default();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let now = Utc::now();
        let issue = |suffix: &str| {
            InferenceCredential::issue(
                InferenceCredentialId::new(),
                organization_id,
                project_id,
                environment_id,
                format!("a3s_inf_{suffix}"),
                VERIFIER,
                now + Duration::hours(2),
                now - Duration::minutes(20),
            )
            .unwrap()
        };
        let first = repository
            .create_inference_credential(issue("aaaaaaaaaaaaaaaa"))
            .await
            .unwrap();
        let second = repository
            .create_inference_credential(issue("bbbbbbbbbbbbbbbb"))
            .await
            .unwrap();
        let active = repository
            .create_inference_credential(issue("cccccccccccccccc"))
            .await
            .unwrap();
        let receipt = |credential: &InferenceCredential, expires_at| {
            InferenceCredentialDeliveryReceipt::new(
                credential.organization_id,
                credential.id,
                credential.generation(),
                EncryptedSecretValue::new("test-key", "encrypted-value").unwrap(),
                expires_at,
                credential.updated_at(),
            )
            .unwrap()
        };
        {
            let mut state = repository.state.write().await;
            state.receipts.insert(
                first.id,
                receipt(&first, now - Duration::minutes(10)),
            );
            state.receipts.insert(
                second.id,
                receipt(&second, now - Duration::minutes(5)),
            );
            state
                .receipts
                .insert(active.id, receipt(&active, now + Duration::minutes(5)));
        }

        assert_eq!(
            repository
                .sweep_expired_inference_credential_delivery_receipts(now, 1)
                .await
                .unwrap(),
            1
        );
        {
            let state = repository.state.read().await;
            assert!(!state.receipts.contains_key(&first.id));
            assert!(state.receipts.contains_key(&second.id));
            assert!(state.receipts.contains_key(&active.id));
            assert!(state.credentials.contains_key(&first.id));
        }
        assert_eq!(
            repository
                .sweep_expired_inference_credential_delivery_receipts(now, 100)
                .await
                .unwrap(),
            1
        );
        let state = repository.state.read().await;
        assert!(!state.receipts.contains_key(&second.id));
        assert!(state.receipts.contains_key(&active.id));
        assert!(state.credentials.contains_key(&second.id));
        assert!(state.credentials.contains_key(&active.id));
    }

    #[tokio::test]
    async fn sweep_rejects_unbounded_limit() {
        let repository = InMemoryInferenceCredentialRepository::default();
        let err = repository
            .sweep_expired_inference_credential_delivery_receipts(Utc::now(), 0)
            .await
            .unwrap_err();
        assert!(matches!(err, RepositoryError::Conflict(_)));
    }
}
