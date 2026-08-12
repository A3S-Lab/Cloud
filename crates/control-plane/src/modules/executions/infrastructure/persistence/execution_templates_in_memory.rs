use crate::modules::executions::domain::{
    CreateExecutionTemplateRevision, ExecutionTemplateRevision, IExecutionTemplateRepository,
};
use crate::modules::shared_kernel::domain::{
    ExecutionTemplateId, ExecutionTemplateRevisionId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryExecutionTemplateRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    revisions: BTreeMap<
        (
            OrganizationId,
            ExecutionTemplateId,
            ExecutionTemplateRevisionId,
        ),
        ExecutionTemplateRevision,
    >,
    idempotency: BTreeMap<(String, String), (String, ExecutionTemplateRevision)>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

impl InMemoryExecutionTemplateRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IExecutionTemplateRepository for InMemoryExecutionTemplateRepository {
    async fn replay_create(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<ExecutionTemplateRevision>>, RepositoryError> {
        let state = self.state.read().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        match state.idempotency.get(&key) {
            Some((digest, _)) if digest != &idempotency.request_digest => {
                Err(RepositoryError::IdempotencyConflict)
            }
            Some((_, revision)) => Ok(Some(IdempotentWrite {
                value: revision.clone(),
                replayed: true,
            })),
            None => Ok(None),
        }
    }

    async fn create(
        &self,
        write: CreateExecutionTemplateRevision,
    ) -> Result<IdempotentWrite<ExecutionTemplateRevision>, RepositoryError> {
        write
            .revision
            .validate()
            .map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, revision)) = state.idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: revision.clone(),
                replayed: true,
            });
        }
        let key = (
            write.revision.organization_id,
            write.revision.template_id,
            write.revision.revision_id,
        );
        if state.revisions.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "execution template revision identity is already in use".into(),
            ));
        }
        state.revisions.insert(key, write.revision.clone());
        state.idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, write.revision.clone()),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.revision,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        template_id: ExecutionTemplateId,
        revision_id: ExecutionTemplateRevisionId,
    ) -> Result<Option<ExecutionTemplateRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .revisions
            .get(&(organization_id, template_id, revision_id))
            .filter(|revision| revision.project_id == project_id)
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<ExecutionTemplateRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut revisions = self
            .state
            .read()
            .await
            .revisions
            .values()
            .filter(|revision| {
                revision.organization_id == organization_id && revision.project_id == project_id
            })
            .cloned()
            .collect::<Vec<_>>();
        revisions.sort_by_key(|revision| {
            std::cmp::Reverse((
                revision.created_at,
                revision.template_id,
                revision.revision_id,
            ))
        });
        revisions.truncate(limit);
        Ok(revisions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::domain::events::ExecutionTemplatePublished;
    use crate::modules::executions::domain::{
        ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplateDefinition,
        ExecutionTemplateDefinitionSpec,
    };
    use crate::modules::shared_kernel::domain::PrincipalId;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn revision(
        organization_id: OrganizationId,
        project_id: ProjectId,
        template_id: ExecutionTemplateId,
        revision_id: ExecutionTemplateRevisionId,
    ) -> ExecutionTemplateRevision {
        let artifact_digest = format!("sha256:{}", "a".repeat(64));
        ExecutionTemplateRevision::create(
            organization_id,
            project_id,
            template_id,
            revision_id,
            ExecutionTemplateDefinition::from_spec(ExecutionTemplateDefinitionSpec {
                name: "repository-test".into(),
                description: "Tests immutable repository identity".into(),
                artifact: ExecutionArtifact {
                    uri: format!("oci://registry.example/tasks/test@{artifact_digest}"),
                    digest: artifact_digest,
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ExecutionProcess {
                    command: vec!["/app/test".into()],
                    args: Vec::new(),
                    working_directory: None,
                    environment: BTreeMap::new(),
                },
                resources: ExecutionResources {
                    cpu_millis: 100,
                    memory_bytes: 64 * 1024 * 1024,
                    pids: 32,
                    ephemeral_storage_bytes: None,
                    timeout_ms: 1_000,
                },
            })
            .expect("definition"),
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("revision")
    }

    fn create_write(
        revision: ExecutionTemplateRevision,
        key: &str,
    ) -> CreateExecutionTemplateRevision {
        let request_id = Uuid::now_v7();
        CreateExecutionTemplateRevision {
            event: ExecutionTemplatePublished::envelope(&revision, request_id)
                .expect("publication event"),
            actor_principal_id: revision.created_by,
            request_id,
            idempotency: IdempotencyRequest::new(
                "execution-template-repository-test",
                key,
                revision.definition.canonical_acl().as_bytes(),
            )
            .expect("idempotency"),
            revision,
        }
    }

    #[tokio::test]
    async fn immutable_identity_cannot_move_between_projects() {
        let repository = InMemoryExecutionTemplateRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let other_project_id = ProjectId::new();
        let template_id = ExecutionTemplateId::new();
        let revision_id = ExecutionTemplateRevisionId::new();
        let first = revision(organization_id, project_id, template_id, revision_id);
        repository
            .create(create_write(first.clone(), "first"))
            .await
            .expect("create first revision");
        let moved = revision(organization_id, other_project_id, template_id, revision_id);
        assert!(matches!(
            repository.create(create_write(moved, "second")).await,
            Err(RepositoryError::Conflict(_))
        ));
        assert_eq!(
            repository
                .find(organization_id, project_id, template_id, revision_id)
                .await
                .expect("find revision"),
            Some(first)
        );
        assert!(repository
            .find(organization_id, other_project_id, template_id, revision_id,)
            .await
            .expect("find other project")
            .is_none());
    }
}
