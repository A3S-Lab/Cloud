use crate::modules::artifacts::domain::entities::{
    PartnerArtifactAdmission, PartnerArtifactAdmissionId,
};
use crate::modules::artifacts::domain::repositories::{
    AdmitPartnerArtifactWrite, IPartnerArtifactAdmissionRepository,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
struct AdmissionState {
    by_id: BTreeMap<(OrganizationId, PartnerArtifactAdmissionId), PartnerArtifactAdmission>,
    by_digest: BTreeMap<(OrganizationId, String), PartnerArtifactAdmissionId>,
    idempotency: BTreeMap<(String, String), (String, Value)>,
}

#[derive(Default)]
pub struct InMemoryPartnerArtifactAdmissionRepository {
    state: RwLock<AdmissionState>,
}

impl InMemoryPartnerArtifactAdmissionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

fn replay<T: DeserializeOwned>(
    state: &AdmissionState,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
) -> Result<Option<IdempotentWrite<T>>, RepositoryError> {
    let key = (idempotency.scope.clone(), idempotency.key.clone());
    let Some((digest, response)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    serde_json::from_value(response.clone())
        .map(|value| {
            Some(IdempotentWrite {
                value,
                replayed: true,
            })
        })
        .map_err(|error| RepositoryError::Storage(error.to_string()))
}

fn remember<T: Serialize>(
    state: &mut AdmissionState,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    value: &T,
) -> Result<(), RepositoryError> {
    let stored =
        serde_json::to_value(value).map_err(|error| RepositoryError::Storage(error.to_string()))?;
    state.idempotency.insert(
        (idempotency.scope.clone(), idempotency.key.clone()),
        (idempotency.request_digest.clone(), stored),
    );
    Ok(())
}

#[async_trait]
impl IPartnerArtifactAdmissionRepository for InMemoryPartnerArtifactAdmissionRepository {
    async fn admit(
        &self,
        write: AdmitPartnerArtifactWrite,
    ) -> Result<IdempotentWrite<PartnerArtifactAdmission>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        let digest_key = (
            write.admission.organization_id,
            write.admission.content_digest.as_str().to_owned(),
        );
        if let Some(existing_id) = state.by_digest.get(&digest_key).copied() {
            let existing = state
                .by_id
                .get(&(write.admission.organization_id, existing_id))
                .cloned()
                .ok_or_else(|| RepositoryError::Storage("digest index is inconsistent".into()))?;
            if !existing.same_facts(&write.admission) {
                return Err(RepositoryError::Conflict(
                    "partner artifact digest is already admitted with different metadata".into(),
                ));
            }
            remember(&mut state, &write.idempotency, &existing)?;
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        state.by_digest.insert(digest_key, write.admission.id);
        state.by_id.insert(
            (write.admission.organization_id, write.admission.id),
            write.admission.clone(),
        );
        remember(&mut state, &write.idempotency, &write.admission)?;
        Ok(IdempotentWrite {
            value: write.admission,
            replayed: false,
        })
    }

    async fn find_by_id(
        &self,
        organization_id: OrganizationId,
        id: PartnerArtifactAdmissionId,
    ) -> Result<Option<PartnerArtifactAdmission>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .by_id
            .get(&(organization_id, id))
            .cloned())
    }

    async fn find_by_digest(
        &self,
        organization_id: OrganizationId,
        content_digest: &Sha256Digest,
    ) -> Result<Option<PartnerArtifactAdmission>, RepositoryError> {
        let state = self.state.read().await;
        let Some(id) = state
            .by_digest
            .get(&(organization_id, content_digest.as_str().to_owned()))
            .copied()
        else {
            return Ok(None);
        };
        Ok(state.by_id.get(&(organization_id, id)).cloned())
    }

    async fn list_by_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PartnerArtifactAdmission>, RepositoryError> {
        let mut items = self
            .state
            .read()
            .await
            .by_id
            .iter()
            .filter(|((org, _), _)| *org == organization_id)
            .map(|(_, admission)| admission.clone())
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then(right.id.as_uuid().cmp(&left.id.as_uuid()))
        });
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::entities::PartnerArtifactKind;
    use crate::modules::shared_kernel::domain::IdempotencyRequest;
    use chrono::Utc;

    fn digest() -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", "ab".repeat(32))).expect("digest")
    }

    fn admission(
        organization_id: OrganizationId,
        kind: PartnerArtifactKind,
        byte_size: u64,
        partner_ref: &str,
    ) -> PartnerArtifactAdmission {
        PartnerArtifactAdmission::create(
            PartnerArtifactAdmissionId::new(),
            organization_id,
            digest(),
            kind,
            byte_size,
            partner_ref.into(),
            Utc::now(),
        )
        .expect("admission")
    }

    fn idempotency(key: &str, body: &str) -> IdempotencyRequest {
        IdempotencyRequest::new(
            "organizations/test/partner-artifact-admissions",
            key,
            body.as_bytes(),
        )
        .expect("idempotency")
    }

    #[tokio::test]
    async fn same_digest_replays_and_metadata_mismatch_conflicts() {
        let repository = InMemoryPartnerArtifactAdmissionRepository::new();
        let organization_id = OrganizationId::new();
        let first = admission(organization_id, PartnerArtifactKind::Model, 12, "weights");
        let created = repository
            .admit(AdmitPartnerArtifactWrite {
                admission: first.clone(),
                idempotency: idempotency("key-1", "body-1"),
            })
            .await
            .expect("admit");
        assert!(!created.replayed);

        let replayed = repository
            .admit(AdmitPartnerArtifactWrite {
                admission: first.clone(),
                idempotency: idempotency("key-1", "body-1"),
            })
            .await
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(replayed.value.id, first.id);

        let same_facts = admission(organization_id, PartnerArtifactKind::Model, 12, "weights");
        let equivalent = repository
            .admit(AdmitPartnerArtifactWrite {
                admission: same_facts,
                idempotency: idempotency("key-2", "body-2"),
            })
            .await
            .expect("same facts");
        assert!(equivalent.replayed);
        assert_eq!(equivalent.value.id, first.id);

        let conflict = repository
            .admit(AdmitPartnerArtifactWrite {
                admission: admission(organization_id, PartnerArtifactKind::Git, 99, "other"),
                idempotency: idempotency("key-3", "body-3"),
            })
            .await;
        assert!(matches!(conflict, Err(RepositoryError::Conflict(_))));
    }
}
