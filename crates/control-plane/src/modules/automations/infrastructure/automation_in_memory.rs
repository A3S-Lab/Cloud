use crate::modules::automations::domain::{
    AppendAutomationRevision, AutomationDefinitionRecord, CreateAutomationDefinition,
    IAutomationDefinitionRepository,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Deterministic local adapter for the Automation definition/revision owner.
/// Production composition uses the PostgreSQL adapter; this implementation is
/// intentionally limited to tests and component-level assembly.
#[derive(Default)]
pub struct InMemoryAutomationDefinitionRepository {
    heads: RwLock<BTreeMap<(Uuid, Uuid), AutomationDefinitionRecord>>,
    revisions: RwLock<BTreeMap<Uuid, a3s_cloud_contracts::AutomationRevisionV1>>,
}

impl InMemoryAutomationDefinitionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IAutomationDefinitionRepository for InMemoryAutomationDefinitionRepository {
    async fn create(
        &self,
        request: CreateAutomationDefinition,
    ) -> Result<AutomationDefinitionRecord, RepositoryError> {
        let record = AutomationDefinitionRecord::new(
            request.definition,
            request.revision,
            request.created_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let key = (
            record.definition.spec().organization_id,
            record.definition.spec().automation_id,
        );
        let revision_id = record.revision.spec().revision_id;
        let mut heads = self.heads.write().await;
        if heads.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "Automation definition already exists".into(),
            ));
        }
        let mut revisions = self.revisions.write().await;
        if revisions.contains_key(&revision_id) {
            return Err(RepositoryError::Conflict(
                "Automation revision already exists".into(),
            ));
        }
        revisions.insert(revision_id, record.revision.clone());
        heads.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        automation_id: Uuid,
    ) -> Result<Option<AutomationDefinitionRecord>, RepositoryError> {
        Ok(self
            .heads
            .read()
            .await
            .get(&(organization_id, automation_id))
            .cloned())
    }

    async fn find_revision(
        &self,
        organization_id: Uuid,
        automation_id: Uuid,
        revision_id: Uuid,
    ) -> Result<Option<a3s_cloud_contracts::AutomationRevisionV1>, RepositoryError> {
        let Some(revision) = self.revisions.read().await.get(&revision_id).cloned() else {
            return Ok(None);
        };
        if revision.spec().definition.organization_id != organization_id
            || revision.spec().definition.automation_id != automation_id
        {
            return Ok(None);
        }
        Ok(Some(revision))
    }

    async fn append_revision(
        &self,
        request: AppendAutomationRevision,
    ) -> Result<AutomationDefinitionRecord, RepositoryError> {
        let key = (request.organization_id, request.automation_id);
        let mut heads = self.heads.write().await;
        let current = heads.get(&key).cloned().ok_or(RepositoryError::NotFound)?;
        let updated = current
            .append(
                request.revision,
                &request.expected_revision_digest,
                request.updated_at,
            )
            .map_err(RepositoryError::Conflict)?;
        let revision_id = updated.revision.spec().revision_id;
        let mut revisions = self.revisions.write().await;
        if revisions.contains_key(&revision_id) {
            return Err(RepositoryError::Conflict(
                "Automation revision already exists".into(),
            ));
        }
        revisions.insert(revision_id, updated.revision.clone());
        heads.insert(key, updated.clone());
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::RepositoryError;
    use a3s_cloud_contracts::AutomationDefinitionV1;
    use chrono::{DateTime, Utc};

    const DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    fn initial() -> (
        AutomationDefinitionV1,
        a3s_cloud_contracts::AutomationRevisionV1,
    ) {
        let definition = AutomationDefinitionV1::parse_acl(DEFINITION).expect("definition");
        let revision = a3s_cloud_contracts::AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x200),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision");
        (definition, revision)
    }

    fn successor(
        parent: &a3s_cloud_contracts::AutomationRevisionV1,
    ) -> a3s_cloud_contracts::AutomationRevisionV1 {
        let mut spec = parent.spec().definition.clone();
        spec.name = "updated-automation".into();
        a3s_cloud_contracts::AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x201),
            2,
            Some(parent),
            spec,
        )
        .expect("successor")
    }

    #[tokio::test]
    async fn persists_current_head_and_historical_revisions() {
        let repository = InMemoryAutomationDefinitionRepository::new();
        let (definition, first) = initial();
        let created = repository
            .create(CreateAutomationDefinition {
                definition,
                revision: first.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        let second = successor(&first);
        let updated = repository
            .append_revision(AppendAutomationRevision {
                organization_id: created.definition.spec().organization_id,
                automation_id: created.definition.spec().automation_id,
                expected_revision_digest: first.digest().into(),
                revision: second.clone(),
                updated_at: timestamp(1_001),
            })
            .await
            .expect("append");
        assert_eq!(updated.revision, second);
        assert_eq!(
            repository
                .find(
                    created.definition.spec().organization_id,
                    created.definition.spec().automation_id,
                )
                .await
                .expect("head lookup")
                .expect("head")
                .revision
                .digest(),
            updated.revision.digest()
        );
        assert_eq!(
            repository
                .find_revision(
                    created.definition.spec().organization_id,
                    created.definition.spec().automation_id,
                    first.spec().revision_id,
                )
                .await
                .expect("revision lookup")
                .expect("first revision"),
            first
        );
        assert!(repository
            .find_revision(
                Uuid::from_u128(0x201),
                created.definition.spec().automation_id,
                first.spec().revision_id,
            )
            .await
            .expect("wrong-organization lookup")
            .is_none());
    }

    #[tokio::test]
    async fn stale_head_and_duplicate_identity_are_rejected() {
        let repository = InMemoryAutomationDefinitionRepository::new();
        let (definition, first) = initial();
        let created = repository
            .create(CreateAutomationDefinition {
                definition,
                revision: first.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        let duplicate = repository
            .create(CreateAutomationDefinition {
                definition: created.definition.clone(),
                revision: first.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect_err("duplicate");
        assert!(
            matches!(duplicate, RepositoryError::Conflict(message) if message.contains("already exists"))
        );
        let stale = repository
            .append_revision(AppendAutomationRevision {
                organization_id: created.definition.spec().organization_id,
                automation_id: created.definition.spec().automation_id,
                expected_revision_digest: "sha256:deadbeef".into(),
                revision: successor(&first),
                updated_at: timestamp(1_001),
            })
            .await
            .expect_err("stale head");
        assert!(matches!(stale, RepositoryError::Conflict(message) if message.contains("stale")));
    }
}
