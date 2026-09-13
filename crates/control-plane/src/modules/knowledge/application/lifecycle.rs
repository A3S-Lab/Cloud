use super::resource_access::{KnowledgeAccess, knowledge_not_found, project};
use crate::modules::knowledge::domain::{
    AppendKnowledgeBaseWrite, CreateKnowledgeBaseWrite, CreateKnowledgePipelineWrite,
    IKnowledgeBaseRepository, IKnowledgePipelineRepository, KnowledgeBaseLifecycleChanged,
    KnowledgeBaseRecord, KnowledgeBaseRevisionV1, KnowledgePipelineLifecycleChanged,
    KnowledgePipelineRecord, KnowledgePipelineReleaseV1, PublishKnowledgePipelineWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

pub const DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT: usize = 50;
pub const MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT: usize = 200;
pub const DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT: usize = 50;
pub const MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMutationResult<T> {
    pub record: T,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgeBaseCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub revision_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct AppendKnowledgeBaseCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: Uuid,
    pub expected_revision_digest: String,
    pub revision_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgePipelineCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub release_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct PublishKnowledgePipelineCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub pipeline_id: Uuid,
    pub expected_release_digest: String,
    pub release_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct GetKnowledgeBase {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: Uuid,
    pub access: KnowledgeAccess,
}

#[derive(Debug, Clone)]
pub struct ListKnowledgeBases {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub limit: Option<usize>,
    pub access: KnowledgeAccess,
}

#[derive(Debug, Clone)]
pub struct GetKnowledgePipeline {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub pipeline_id: Uuid,
    pub access: KnowledgeAccess,
}

#[derive(Debug, Clone)]
pub struct ListKnowledgePipelines {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub limit: Option<usize>,
    pub access: KnowledgeAccess,
}

/// Authorized Knowledge catalog lifecycle boundary.
///
/// Authorization precedes replay. Creates/appends/publishes are exact digest-
/// fenced writes with shared idempotency, audit, and Outbox side effects owned
/// by the repository adapters. Authorized reads share the same KnowledgeAccess
/// projection.
#[derive(Clone)]
pub struct KnowledgeCatalogLifecycleService {
    bases: Arc<dyn IKnowledgeBaseRepository>,
    pipelines: Arc<dyn IKnowledgePipelineRepository>,
}

impl KnowledgeCatalogLifecycleService {
    pub fn new(
        bases: Arc<dyn IKnowledgeBaseRepository>,
        pipelines: Arc<dyn IKnowledgePipelineRepository>,
    ) -> Self {
        Self { bases, pipelines }
    }

    pub async fn create_knowledge_base(
        &self,
        command: CreateKnowledgeBaseCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgeBaseRecord>> {
        project(command.project_id, &command.access)?;
        let revision = KnowledgeBaseRevisionV1::parse_acl(&command.revision_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = revision.spec();
        if spec.organization_id != command.organization_id || spec.project_id != command.project_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgeBase revision ACL is outside the requested tenant scope".into(),
            ));
        }
        let canonical = serde_json::to_vec(&CanonicalCreateKnowledgeBase {
            organization_id: command.organization_id,
            project_id: command.project_id,
            knowledge_base_id: spec.knowledge_base_id.as_uuid(),
            revision_digest: revision.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-bases",
                command.organization_id, command.project_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.bases.replay_write(&idempotency).await? {
            if !create_base_replay_matches(&record, &command, &revision) {
                return Err(ApplicationError::Internal(
                    "KnowledgeBase create replay reference is inconsistent".into(),
                ));
            }
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record = KnowledgeBaseRecord::new(revision, now).map_err(ApplicationError::Invalid)?;
        let event = KnowledgeBaseLifecycleChanged::created(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .bases
            .create_write(CreateKnowledgeBaseWrite {
                record,
                event,
                actor_principal_id: command.actor_principal_id,
                request_id: command.request_id,
                idempotency,
            })
            .await?;
        Ok(KnowledgeMutationResult {
            record: written.value,
            replayed: written.replayed,
        })
    }

    pub async fn append_knowledge_base(
        &self,
        command: AppendKnowledgeBaseCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgeBaseRecord>> {
        project(command.project_id, &command.access)?;
        let revision = KnowledgeBaseRevisionV1::parse_acl(&command.revision_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = revision.spec();
        if spec.organization_id != command.organization_id
            || spec.project_id != command.project_id
            || spec.knowledge_base_id.as_uuid() != command.knowledge_base_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgeBase revision ACL is outside the requested tenant scope".into(),
            ));
        }
        let current = self
            .bases
            .find(command.organization_id.as_uuid(), command.knowledge_base_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if current.revision.spec().project_id != command.project_id {
            return Err(knowledge_not_found());
        }
        let canonical = serde_json::to_vec(&CanonicalAppendKnowledgeBase {
            organization_id: command.organization_id,
            project_id: command.project_id,
            knowledge_base_id: command.knowledge_base_id,
            expected_revision_digest: &command.expected_revision_digest,
            revision_digest: revision.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-bases/{}/revisions",
                command.organization_id, command.project_id, command.knowledge_base_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.bases.replay_write(&idempotency).await? {
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record = current
            .append(revision, &command.expected_revision_digest, now)
            .map_err(ApplicationError::Conflict)?;
        let event = KnowledgeBaseLifecycleChanged::revised(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .bases
            .append_write(AppendKnowledgeBaseWrite {
                record,
                expected_revision_digest: command.expected_revision_digest,
                event,
                actor_principal_id: command.actor_principal_id,
                request_id: command.request_id,
                idempotency,
            })
            .await?;
        Ok(KnowledgeMutationResult {
            record: written.value,
            replayed: written.replayed,
        })
    }

    pub async fn create_knowledge_pipeline(
        &self,
        command: CreateKnowledgePipelineCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgePipelineRecord>> {
        project(command.project_id, &command.access)?;
        let release = KnowledgePipelineReleaseV1::parse_acl(&command.release_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = release.spec();
        if spec.organization_id != command.organization_id || spec.project_id != command.project_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgePipeline release ACL is outside the requested tenant scope".into(),
            ));
        }
        let canonical = serde_json::to_vec(&CanonicalCreateKnowledgePipeline {
            organization_id: command.organization_id,
            project_id: command.project_id,
            pipeline_id: spec.pipeline_id.as_uuid(),
            release_digest: release.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-pipelines",
                command.organization_id, command.project_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.pipelines.replay_write(&idempotency).await? {
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record =
            KnowledgePipelineRecord::new(release, now).map_err(ApplicationError::Invalid)?;
        let event = KnowledgePipelineLifecycleChanged::created(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .pipelines
            .create_write(CreateKnowledgePipelineWrite {
                record,
                event,
                actor_principal_id: command.actor_principal_id,
                request_id: command.request_id,
                idempotency,
            })
            .await?;
        Ok(KnowledgeMutationResult {
            record: written.value,
            replayed: written.replayed,
        })
    }

    pub async fn publish_knowledge_pipeline(
        &self,
        command: PublishKnowledgePipelineCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgePipelineRecord>> {
        project(command.project_id, &command.access)?;
        let release = KnowledgePipelineReleaseV1::parse_acl(&command.release_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = release.spec();
        if spec.organization_id != command.organization_id
            || spec.project_id != command.project_id
            || spec.pipeline_id.as_uuid() != command.pipeline_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgePipeline release ACL is outside the requested tenant scope".into(),
            ));
        }
        let current = self
            .pipelines
            .find(command.organization_id.as_uuid(), command.pipeline_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if current.release.spec().project_id != command.project_id {
            return Err(knowledge_not_found());
        }
        let canonical = serde_json::to_vec(&CanonicalPublishKnowledgePipeline {
            organization_id: command.organization_id,
            project_id: command.project_id,
            pipeline_id: command.pipeline_id,
            expected_release_digest: &command.expected_release_digest,
            release_digest: release.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-pipelines/{}/releases",
                command.organization_id, command.project_id, command.pipeline_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.pipelines.replay_write(&idempotency).await? {
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record = current
            .publish(release, &command.expected_release_digest, now)
            .map_err(ApplicationError::Conflict)?;
        let event = KnowledgePipelineLifecycleChanged::published(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .pipelines
            .publish_write(PublishKnowledgePipelineWrite {
                record,
                expected_release_digest: command.expected_release_digest,
                event,
                actor_principal_id: command.actor_principal_id,
                request_id: command.request_id,
                idempotency,
            })
            .await?;
        Ok(KnowledgeMutationResult {
            record: written.value,
            replayed: written.replayed,
        })
    }

    pub async fn get_knowledge_base(
        &self,
        query: GetKnowledgeBase,
    ) -> ApplicationResult<KnowledgeBaseRecord> {
        project(query.project_id, &query.access)?;
        let record = self
            .bases
            .find(query.organization_id.as_uuid(), query.knowledge_base_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if record.revision.spec().project_id != query.project_id {
            return Err(knowledge_not_found());
        }
        Ok(record)
    }

    pub async fn list_knowledge_bases(
        &self,
        query: ListKnowledgeBases,
    ) -> ApplicationResult<Vec<KnowledgeBaseRecord>> {
        project(query.project_id, &query.access)?;
        let limit = query.limit.unwrap_or(DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT);
        if limit == 0 || limit > MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT {
            return Err(ApplicationError::Invalid(format!(
                "KnowledgeBase list limit must be between 1 and {MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT}"
            )));
        }
        self.bases
            .list_for_project(
                query.organization_id.as_uuid(),
                query.project_id.as_uuid(),
                limit,
            )
            .await
            .map_err(Into::into)
    }

    pub async fn get_knowledge_pipeline(
        &self,
        query: GetKnowledgePipeline,
    ) -> ApplicationResult<KnowledgePipelineRecord> {
        project(query.project_id, &query.access)?;
        let record = self
            .pipelines
            .find(query.organization_id.as_uuid(), query.pipeline_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if record.release.spec().project_id != query.project_id {
            return Err(knowledge_not_found());
        }
        Ok(record)
    }

    pub async fn list_knowledge_pipelines(
        &self,
        query: ListKnowledgePipelines,
    ) -> ApplicationResult<Vec<KnowledgePipelineRecord>> {
        project(query.project_id, &query.access)?;
        let limit = query.limit.unwrap_or(DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT);
        if limit == 0 || limit > MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT {
            return Err(ApplicationError::Invalid(format!(
                "KnowledgePipeline list limit must be between 1 and {MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT}"
            )));
        }
        self.pipelines
            .list(
                query.organization_id.as_uuid(),
                query.project_id.as_uuid(),
                limit,
            )
            .await
            .map_err(Into::into)
    }
}

fn create_base_replay_matches(
    record: &KnowledgeBaseRecord,
    command: &CreateKnowledgeBaseCommand,
    revision: &KnowledgeBaseRevisionV1,
) -> bool {
    let spec = record.revision.spec();
    spec.organization_id == command.organization_id
        && spec.project_id == command.project_id
        && record.revision.digest().as_str() == revision.digest().as_str()
}

fn canonical_now() -> ApplicationResult<chrono::DateTime<Utc>> {
    let now = Utc::now();
    truncate_to_seconds(now).ok_or_else(|| {
        ApplicationError::Internal("Knowledge catalog timestamps require whole seconds".into())
    })
}

fn truncate_to_seconds(value: chrono::DateTime<Utc>) -> Option<chrono::DateTime<Utc>> {
    chrono::DateTime::from_timestamp(value.timestamp(), 0)
}

#[derive(Serialize)]
struct CanonicalCreateKnowledgeBase<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    knowledge_base_id: Uuid,
    revision_digest: &'a str,
}

#[derive(Serialize)]
struct CanonicalAppendKnowledgeBase<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    knowledge_base_id: Uuid,
    expected_revision_digest: &'a str,
    revision_digest: &'a str,
}

#[derive(Serialize)]
struct CanonicalCreateKnowledgePipeline<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    pipeline_id: Uuid,
    release_digest: &'a str,
}

#[derive(Serialize)]
struct CanonicalPublishKnowledgePipeline<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    pipeline_id: Uuid,
    expected_release_digest: &'a str,
    release_digest: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::domain::{
        IKnowledgeBaseRepository, IKnowledgePipelineRepository, KnowledgeBaseRevisionV1,
    };
    use crate::modules::knowledge::infrastructure::{
        InMemoryKnowledgeBaseRepository, InMemoryKnowledgePipelineRepository,
    };

    const BASE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-base-revision.acl"
    ));

    #[tokio::test]
    async fn authorized_create_replays_exactly_once() {
        let bases = Arc::new(InMemoryKnowledgeBaseRepository::new());
        let pipelines = Arc::new(InMemoryKnowledgePipelineRepository::new());
        let service = KnowledgeCatalogLifecycleService::new(bases, pipelines);
        let revision = KnowledgeBaseRevisionV1::parse_acl(BASE).expect("revision");
        let command = CreateKnowledgeBaseCommand {
            organization_id: revision.spec().organization_id,
            project_id: revision.spec().project_id,
            revision_acl: BASE.to_owned(),
            actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
            access: KnowledgeAccess::restricted_projects([revision.spec().project_id]),
            idempotency_key: "create-1".into(),
            request_id: Uuid::from_u128(0x88),
        };
        let first = service
            .create_knowledge_base(command.clone())
            .await
            .expect("create");
        assert!(!first.replayed);
        let second = service
            .create_knowledge_base(command)
            .await
            .expect("replay");
        assert!(second.replayed);
        assert_eq!(first.record.revision, second.record.revision);

        let denied = service
            .create_knowledge_base(CreateKnowledgeBaseCommand {
                organization_id: revision.spec().organization_id,
                project_id: revision.spec().project_id,
                revision_acl: BASE.to_owned(),
                actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
                access: KnowledgeAccess::restricted_projects([ProjectId::new()]),
                idempotency_key: "create-2".into(),
                request_id: Uuid::from_u128(0x89),
            })
            .await;
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn authorized_reads_require_project_visibility() {
        let bases = Arc::new(InMemoryKnowledgeBaseRepository::new());
        let pipelines = Arc::new(InMemoryKnowledgePipelineRepository::new());
        let service = KnowledgeCatalogLifecycleService::new(
            bases as Arc<dyn IKnowledgeBaseRepository>,
            pipelines as Arc<dyn IKnowledgePipelineRepository>,
        );
        let revision = KnowledgeBaseRevisionV1::parse_acl(BASE).expect("revision");
        let created = service
            .create_knowledge_base(CreateKnowledgeBaseCommand {
                organization_id: revision.spec().organization_id,
                project_id: revision.spec().project_id,
                revision_acl: BASE.to_owned(),
                actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
                access: KnowledgeAccess::restricted_projects([revision.spec().project_id]),
                idempotency_key: "create-read-1".into(),
                request_id: Uuid::from_u128(0x8a),
            })
            .await
            .expect("create");

        let listed = service
            .list_knowledge_bases(ListKnowledgeBases {
                organization_id: revision.spec().organization_id,
                project_id: revision.spec().project_id,
                limit: Some(10),
                access: KnowledgeAccess::restricted_projects([revision.spec().project_id]),
            })
            .await
            .expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].revision.digest().as_str(),
            created.record.revision.digest().as_str()
        );

        let fetched = service
            .get_knowledge_base(GetKnowledgeBase {
                organization_id: revision.spec().organization_id,
                project_id: revision.spec().project_id,
                knowledge_base_id: revision.spec().knowledge_base_id.as_uuid(),
                access: KnowledgeAccess::restricted_projects([revision.spec().project_id]),
            })
            .await
            .expect("get");
        assert_eq!(fetched.revision, created.record.revision);

        let denied = service
            .get_knowledge_base(GetKnowledgeBase {
                organization_id: revision.spec().organization_id,
                project_id: revision.spec().project_id,
                knowledge_base_id: revision.spec().knowledge_base_id.as_uuid(),
                access: KnowledgeAccess::restricted_projects([ProjectId::new()]),
            })
            .await;
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    }
}
