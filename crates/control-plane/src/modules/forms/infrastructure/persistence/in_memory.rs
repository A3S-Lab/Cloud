use super::validation::{
    form_name_key, validate_initial_draft, validate_publication, validate_publication_record,
    validate_revision,
};
use crate::modules::forms::domain::{
    CreateFormDraftWrite, FormDraft, FormPublicationRecord, FormRelease, IFormRepository,
    PublishFormReleaseWrite, ReviseFormDraftWrite,
};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryFormRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    drafts: BTreeMap<(OrganizationId, FormId), FormDraft>,
    names: BTreeMap<(OrganizationId, ProjectId, String), FormId>,
    releases: BTreeMap<(OrganizationId, FormId, FormReleaseId), FormRelease>,
    idempotency: BTreeMap<(String, String), (String, serde_json::Value)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryFormRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IFormRepository for InMemoryFormRepository {
    async fn replay_draft_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<FormDraft>>, RepositoryError> {
        let state = self.state.read().await;
        let replay = replay::<FormDraft>(&state, idempotency)?;
        if let Some(replay) = &replay {
            replay.value.validate().map_err(|error| {
                RepositoryError::Storage(format!("Form draft replay target is invalid: {error}"))
            })?;
        }
        Ok(replay)
    }

    async fn replay_publication(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<FormPublicationRecord>>, RepositoryError> {
        let state = self.state.read().await;
        let replay = replay::<FormPublicationRecord>(&state, idempotency)?;
        if let Some(replay) = &replay {
            validate_publication_record(&replay.value)?;
        }
        Ok(replay)
    }

    async fn create_draft(
        &self,
        write: CreateFormDraftWrite,
    ) -> Result<IdempotentWrite<FormDraft>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(replay) = replay::<FormDraft>(&state, &write.idempotency)? {
            replay.value.validate().map_err(RepositoryError::Storage)?;
            return Ok(replay);
        }
        validate_initial_draft(&write.draft, write.actor_principal_id)?;
        let draft = &write.draft;
        let key = (draft.organization_id, draft.id);
        let name_key = (
            draft.organization_id,
            draft.project_id,
            form_name_key(&draft.name),
        );
        if state.drafts.contains_key(&key) || state.names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "Form name is already in use in this project".into(),
            ));
        }
        state.names.insert(name_key, draft.id);
        state.drafts.insert(key, draft.clone());
        store_replay(&mut state, &write.idempotency, draft)?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.draft,
            replayed: false,
        })
    }

    async fn revise_draft(
        &self,
        write: ReviseFormDraftWrite,
    ) -> Result<IdempotentWrite<FormDraft>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(replay) = replay::<FormDraft>(&state, &write.idempotency)? {
            replay.value.validate().map_err(RepositoryError::Storage)?;
            return Ok(replay);
        }
        let key = (write.draft.organization_id, write.draft.id);
        let current = state
            .drafts
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        validate_revision(
            &current,
            &write.draft,
            write.expected_version,
            write.actor_principal_id,
        )?;
        let old_name_key = (
            current.organization_id,
            current.project_id,
            form_name_key(&current.name),
        );
        let new_name_key = (
            write.draft.organization_id,
            write.draft.project_id,
            form_name_key(&write.draft.name),
        );
        if old_name_key != new_name_key && state.names.contains_key(&new_name_key) {
            return Err(RepositoryError::Conflict(
                "Form name is already in use in this project".into(),
            ));
        }
        if old_name_key != new_name_key {
            state.names.remove(&old_name_key);
            state.names.insert(new_name_key, write.draft.id);
        }
        state.drafts.insert(key, write.draft.clone());
        store_replay(&mut state, &write.idempotency, &write.draft)?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.draft,
            replayed: false,
        })
    }

    async fn publish_release(
        &self,
        write: PublishFormReleaseWrite,
    ) -> Result<IdempotentWrite<FormPublicationRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(replay) = replay::<FormPublicationRecord>(&state, &write.idempotency)? {
            validate_publication_record(&replay.value)?;
            return Ok(replay);
        }
        let draft_key = (
            write.publication.draft.organization_id,
            write.publication.draft.id,
        );
        let current = state
            .drafts
            .get(&draft_key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        validate_publication(
            &current,
            &write.publication,
            write.expected_version,
            write.actor_principal_id,
        )?;
        if state.releases.values().any(|release| {
            release.organization_id == write.publication.release.organization_id
                && release.form_id == write.publication.release.form_id
                && (release.id == write.publication.release.id
                    || release.revision == write.publication.release.revision
                    || release.source_draft_version
                        == write.publication.release.source_draft_version)
        }) {
            return Err(RepositoryError::Conflict(
                "Form draft version already has a published release".into(),
            ));
        }
        let release = &write.publication.release;
        state.releases.insert(
            (release.organization_id, release.form_id, release.id),
            release.clone(),
        );
        state
            .drafts
            .insert(draft_key, write.publication.draft.clone());
        store_replay(&mut state, &write.idempotency, &write.publication)?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.publication,
            replayed: false,
        })
    }

    async fn find_draft(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
    ) -> Result<Option<FormDraft>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .drafts
            .get(&(organization_id, form_id))
            .cloned())
    }

    async fn list_drafts(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<FormDraft>, RepositoryError> {
        let mut values = self
            .state
            .read()
            .await
            .drafts
            .values()
            .filter(|draft| {
                draft.organization_id == organization_id && draft.project_id == project_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            form_name_key(&left.name)
                .cmp(&form_name_key(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(values)
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
        release_id: FormReleaseId,
    ) -> Result<Option<FormRelease>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .releases
            .get(&(organization_id, form_id, release_id))
            .cloned())
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
    ) -> Result<Vec<FormRelease>, RepositoryError> {
        let mut values = self
            .state
            .read()
            .await
            .releases
            .values()
            .filter(|release| {
                release.organization_id == organization_id && release.form_id == form_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .revision
                .cmp(&left.revision)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(values)
    }
}

fn replay<T: DeserializeOwned>(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotentWrite<T>>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((request_digest, response)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if request_digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    let value = serde_json::from_value(response.clone()).map_err(|error| {
        RepositoryError::Storage(format!("Form idempotency response is invalid: {error}"))
    })?;
    Ok(Some(IdempotentWrite {
        value,
        replayed: true,
    }))
}

fn store_replay<T: Serialize>(
    state: &mut State,
    idempotency: &IdempotencyRequest,
    response: &T,
) -> Result<(), RepositoryError> {
    let response = serde_json::to_value(response).map_err(|error| {
        RepositoryError::Storage(format!(
            "Form idempotency response could not encode: {error}"
        ))
    })?;
    state.idempotency.insert(
        (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        ),
        (idempotency.request_digest.clone(), response),
    );
    Ok(())
}
