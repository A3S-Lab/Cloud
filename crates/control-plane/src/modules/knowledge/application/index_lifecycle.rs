use super::lifecycle::KnowledgeMutationResult;
use super::resource_access::{knowledge_not_found, project, KnowledgeAccess};
use crate::modules::knowledge::domain::{
    CreateExternalKnowledgeBindingWrite, CreateKnowledgeIndexRevisionWrite,
    CreateKnowledgeRetrievalPolicyRevisionWrite, ExternalKnowledgeBindingLifecycleChanged,
    ExternalKnowledgeBindingRecord, ExternalKnowledgeBindingV1, IExternalKnowledgeBindingRepository,
    IKnowledgeIndexRevisionRepository, IKnowledgeRetrievalPolicyRevisionRepository,
    KnowledgeIndexLifecycleChanged, KnowledgeIndexRevisionRecord, KnowledgeIndexRevisionV1,
    KnowledgeRetrievalPolicyLifecycleChanged, KnowledgeRetrievalPolicyRevisionRecord,
    KnowledgeRetrievalPolicyRevisionV1,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateKnowledgeIndexRevisionCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub index_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgeRetrievalPolicyRevisionCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub policy_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CreateExternalKnowledgeBindingCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub binding_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}


#[derive(Debug, Clone)]
pub struct GetKnowledgeIndexRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub index_revision_id: Uuid,
    pub access: KnowledgeAccess,
}

#[derive(Debug, Clone)]
pub struct GetKnowledgeRetrievalPolicyRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub policy_revision_id: Uuid,
    pub access: KnowledgeAccess,
}

#[derive(Debug, Clone)]
pub struct GetExternalKnowledgeBinding {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub binding_id: Uuid,
    pub access: KnowledgeAccess,
}
/// Authorized Knowledge index/policy/binding lifecycle boundary.
#[derive(Clone)]
pub struct KnowledgeIndexLifecycleService {
    indexes: Arc<dyn IKnowledgeIndexRevisionRepository>,
    policies: Arc<dyn IKnowledgeRetrievalPolicyRevisionRepository>,
    bindings: Arc<dyn IExternalKnowledgeBindingRepository>,
}

impl KnowledgeIndexLifecycleService {
    pub fn new(
        indexes: Arc<dyn IKnowledgeIndexRevisionRepository>,
        policies: Arc<dyn IKnowledgeRetrievalPolicyRevisionRepository>,
        bindings: Arc<dyn IExternalKnowledgeBindingRepository>,
    ) -> Self {
        Self {
            indexes,
            policies,
            bindings,
        }
    }

    pub async fn create_index_revision(
        &self,
        command: CreateKnowledgeIndexRevisionCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgeIndexRevisionRecord>> {
        project(command.project_id, &command.access)?;
        let index = KnowledgeIndexRevisionV1::parse_acl(&command.index_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = index.spec();
        if spec.organization_id != command.organization_id || spec.project_id != command.project_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgeIndexRevision ACL is outside the requested tenant scope".into(),
            ));
        }
        let canonical = serde_json::to_vec(&CanonicalCreateKnowledgeIndex {
            organization_id: command.organization_id,
            project_id: command.project_id,
            index_revision_id: spec.index_revision_id.as_uuid(),
            index_digest: index.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-index-revisions",
                command.organization_id, command.project_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.indexes.replay_write(&idempotency).await? {
            if !create_index_replay_matches(&record, &command, &index) {
                return Err(ApplicationError::Internal(
                    "KnowledgeIndexRevision create replay reference is inconsistent".into(),
                ));
            }
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record =
            KnowledgeIndexRevisionRecord::new(index, now).map_err(ApplicationError::Invalid)?;
        let event = KnowledgeIndexLifecycleChanged::created(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .indexes
            .create_write(CreateKnowledgeIndexRevisionWrite {
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

    pub async fn create_retrieval_policy_revision(
        &self,
        command: CreateKnowledgeRetrievalPolicyRevisionCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgeRetrievalPolicyRevisionRecord>> {
        project(command.project_id, &command.access)?;
        let policy = KnowledgeRetrievalPolicyRevisionV1::parse_acl(&command.policy_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = policy.spec();
        if spec.organization_id != command.organization_id || spec.project_id != command.project_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgeRetrievalPolicyRevision ACL is outside the requested tenant scope".into(),
            ));
        }
        let canonical = serde_json::to_vec(&CanonicalCreateKnowledgeRetrievalPolicy {
            organization_id: command.organization_id,
            project_id: command.project_id,
            policy_revision_id: spec.policy_revision_id.as_uuid(),
            policy_digest: policy.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-retrieval-policy-revisions",
                command.organization_id, command.project_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.policies.replay_write(&idempotency).await? {
            if !create_policy_replay_matches(&record, &command, &policy) {
                return Err(ApplicationError::Internal(
                    "KnowledgeRetrievalPolicyRevision create replay reference is inconsistent"
                        .into(),
                ));
            }
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record = KnowledgeRetrievalPolicyRevisionRecord::new(policy, now)
            .map_err(ApplicationError::Invalid)?;
        let event = KnowledgeRetrievalPolicyLifecycleChanged::created(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .policies
            .create_write(CreateKnowledgeRetrievalPolicyRevisionWrite {
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

    pub async fn create_external_binding(
        &self,
        command: CreateExternalKnowledgeBindingCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<ExternalKnowledgeBindingRecord>> {
        project(command.project_id, &command.access)?;
        let binding = ExternalKnowledgeBindingV1::parse_acl(&command.binding_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = binding.spec();
        if spec.organization_id != command.organization_id || spec.project_id != command.project_id
        {
            return Err(ApplicationError::Invalid(
                "ExternalKnowledgeBinding ACL is outside the requested tenant scope".into(),
            ));
        }
        let canonical = serde_json::to_vec(&CanonicalCreateExternalKnowledgeBinding {
            organization_id: command.organization_id,
            project_id: command.project_id,
            binding_id: spec.binding_id.as_uuid(),
            binding_digest: binding.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/external-knowledge-bindings",
                command.organization_id, command.project_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.bindings.replay_write(&idempotency).await? {
            if !create_binding_replay_matches(&record, &command, &binding) {
                return Err(ApplicationError::Internal(
                    "ExternalKnowledgeBinding create replay reference is inconsistent".into(),
                ));
            }
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record =
            ExternalKnowledgeBindingRecord::new(binding, now).map_err(ApplicationError::Invalid)?;
        let event = ExternalKnowledgeBindingLifecycleChanged::created(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .bindings
            .create_write(CreateExternalKnowledgeBindingWrite {
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

    pub async fn get_index_revision(
        &self,
        query: GetKnowledgeIndexRevision,
    ) -> ApplicationResult<KnowledgeIndexRevisionRecord> {
        project(query.project_id, &query.access)?;
        let record = self
            .indexes
            .find(query.organization_id.as_uuid(), query.index_revision_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if record.index_revision.spec().project_id != query.project_id {
            return Err(knowledge_not_found());
        }
        Ok(record)
    }

    pub async fn get_retrieval_policy_revision(
        &self,
        query: GetKnowledgeRetrievalPolicyRevision,
    ) -> ApplicationResult<KnowledgeRetrievalPolicyRevisionRecord> {
        project(query.project_id, &query.access)?;
        let record = self
            .policies
            .find(query.organization_id.as_uuid(), query.policy_revision_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if record.policy_revision.spec().project_id != query.project_id {
            return Err(knowledge_not_found());
        }
        Ok(record)
    }

    pub async fn get_external_binding(
        &self,
        query: GetExternalKnowledgeBinding,
    ) -> ApplicationResult<ExternalKnowledgeBindingRecord> {
        project(query.project_id, &query.access)?;
        let record = self
            .bindings
            .find(query.organization_id.as_uuid(), query.binding_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if record.binding.spec().project_id != query.project_id {
            return Err(knowledge_not_found());
        }
        Ok(record)
    }
}


fn canonical_now() -> ApplicationResult<chrono::DateTime<Utc>> {
    let now = Utc::now();
    chrono::DateTime::from_timestamp(now.timestamp(), 0).ok_or_else(|| {
        ApplicationError::Internal("Knowledge catalog timestamps require whole seconds".into())
    })
}

fn create_index_replay_matches(
    record: &KnowledgeIndexRevisionRecord,
    command: &CreateKnowledgeIndexRevisionCommand,
    index: &KnowledgeIndexRevisionV1,
) -> bool {
    let spec = record.index_revision.spec();
    spec.organization_id == command.organization_id
        && spec.project_id == command.project_id
        && record.index_revision.digest() == index.digest()
        && record.index_revision.canonical_acl() == index.canonical_acl()
}

fn create_policy_replay_matches(
    record: &KnowledgeRetrievalPolicyRevisionRecord,
    command: &CreateKnowledgeRetrievalPolicyRevisionCommand,
    policy: &KnowledgeRetrievalPolicyRevisionV1,
) -> bool {
    let spec = record.policy_revision.spec();
    spec.organization_id == command.organization_id
        && spec.project_id == command.project_id
        && record.policy_revision.digest() == policy.digest()
        && record.policy_revision.canonical_acl() == policy.canonical_acl()
}

fn create_binding_replay_matches(
    record: &ExternalKnowledgeBindingRecord,
    command: &CreateExternalKnowledgeBindingCommand,
    binding: &ExternalKnowledgeBindingV1,
) -> bool {
    let spec = record.binding.spec();
    spec.organization_id == command.organization_id
        && spec.project_id == command.project_id
        && record.binding.digest() == binding.digest()
        && record.binding.canonical_acl() == binding.canonical_acl()
}

#[derive(Serialize)]
struct CanonicalCreateKnowledgeIndex<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    index_revision_id: Uuid,
    index_digest: &'a str,
}

#[derive(Serialize)]
struct CanonicalCreateKnowledgeRetrievalPolicy<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    policy_revision_id: Uuid,
    policy_digest: &'a str,
}

#[derive(Serialize)]
struct CanonicalCreateExternalKnowledgeBinding<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    binding_id: Uuid,
    binding_digest: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::infrastructure::{
        InMemoryExternalKnowledgeBindingRepository, InMemoryKnowledgeIndexRevisionRepository,
        InMemoryKnowledgeRetrievalPolicyRevisionRepository,
    };

    const INDEX: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-index-revision.acl"
    ));
    const POLICY: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-retrieval-policy-revision.acl"
    ));
    const BINDING: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/external-knowledge-binding.acl"
    ));

    fn service() -> KnowledgeIndexLifecycleService {
        KnowledgeIndexLifecycleService::new(
            Arc::new(InMemoryKnowledgeIndexRevisionRepository::new()),
            Arc::new(InMemoryKnowledgeRetrievalPolicyRevisionRepository::new()),
            Arc::new(InMemoryExternalKnowledgeBindingRepository::new()),
        )
    }

    #[tokio::test]
    async fn authorized_index_create_replays_exactly_once() {
        let service = service();
        let index = KnowledgeIndexRevisionV1::parse_acl(INDEX).expect("index");
        let command = CreateKnowledgeIndexRevisionCommand {
            organization_id: index.spec().organization_id,
            project_id: index.spec().project_id,
            index_acl: INDEX.to_owned(),
            actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
            access: KnowledgeAccess::restricted_projects([index.spec().project_id]),
            idempotency_key: "create-index-1".into(),
            request_id: Uuid::from_u128(0x91),
        };
        let first = service
            .create_index_revision(command.clone())
            .await
            .expect("create");
        assert!(!first.replayed);
        let second = service
            .create_index_revision(command)
            .await
            .expect("replay");
        assert!(second.replayed);
        assert_eq!(first.record.index_revision, second.record.index_revision);

        let denied = service
            .create_index_revision(CreateKnowledgeIndexRevisionCommand {
                organization_id: index.spec().organization_id,
                project_id: index.spec().project_id,
                index_acl: INDEX.to_owned(),
                actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
                access: KnowledgeAccess::restricted_projects([ProjectId::new()]),
                idempotency_key: "create-index-2".into(),
                request_id: Uuid::from_u128(0x92),
            })
            .await;
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn authorized_policy_and_binding_create_replay() {
        let service = service();
        let policy = KnowledgeRetrievalPolicyRevisionV1::parse_acl(POLICY).expect("policy");
        let policy_command = CreateKnowledgeRetrievalPolicyRevisionCommand {
            organization_id: policy.spec().organization_id,
            project_id: policy.spec().project_id,
            policy_acl: POLICY.to_owned(),
            actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
            access: KnowledgeAccess::restricted_projects([policy.spec().project_id]),
            idempotency_key: "create-policy-1".into(),
            request_id: Uuid::from_u128(0x93),
        };
        let first = service
            .create_retrieval_policy_revision(policy_command.clone())
            .await
            .expect("create policy");
        assert!(!first.replayed);
        let second = service
            .create_retrieval_policy_revision(policy_command)
            .await
            .expect("replay policy");
        assert!(second.replayed);

        let binding = ExternalKnowledgeBindingV1::parse_acl(BINDING).expect("binding");
        let binding_command = CreateExternalKnowledgeBindingCommand {
            organization_id: binding.spec().organization_id,
            project_id: binding.spec().project_id,
            binding_acl: BINDING.to_owned(),
            actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
            access: KnowledgeAccess::restricted_projects([binding.spec().project_id]),
            idempotency_key: "create-binding-1".into(),
            request_id: Uuid::from_u128(0x94),
        };
        let first = service
            .create_external_binding(binding_command.clone())
            .await
            .expect("create binding");
        assert!(!first.replayed);
        let second = service
            .create_external_binding(binding_command)
            .await
            .expect("replay binding");
        assert!(second.replayed);
        assert_eq!(first.record.binding, second.record.binding);
    }
}
