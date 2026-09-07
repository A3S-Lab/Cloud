use crate::modules::automations::domain::{
    AppendAutomationRevision, AutomationDefinitionRecord, CreateAutomationDefinition,
    IAutomationDefinitionRepository,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use std::sync::Arc;
use uuid::Uuid;

/// Application owner boundary for immutable Automation definitions.
///
/// Presentation and worker composition use this service instead of reaching
/// into a persistence adapter. It deliberately exposes no mutable update:
/// every change is an exact, digest-fenced revision append.
#[derive(Clone)]
pub struct AutomationDefinitionCatalogService {
    repository: Arc<dyn IAutomationDefinitionRepository>,
}

impl AutomationDefinitionCatalogService {
    pub fn new(repository: Arc<dyn IAutomationDefinitionRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateAutomationDefinition,
    ) -> ApplicationResult<AutomationDefinitionRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        automation_id: Uuid,
    ) -> ApplicationResult<Option<AutomationDefinitionRecord>> {
        self.repository
            .find(organization_id, automation_id)
            .await
            .map_err(Into::into)
    }

    pub async fn list(&self, limit: usize) -> ApplicationResult<Vec<AutomationDefinitionRecord>> {
        self.repository.list(limit).await.map_err(Into::into)
    }

    pub async fn append_revision(
        &self,
        request: AppendAutomationRevision,
    ) -> ApplicationResult<AutomationDefinitionRecord> {
        self.repository
            .append_revision(request)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::automations::infrastructure::InMemoryAutomationDefinitionRepository;
    use a3s_cloud_contracts::{AutomationDefinitionV1, AutomationRevisionV1};
    use chrono::DateTime;

    const SCHEDULE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-schedule.acl"
    ));

    fn timestamp(seconds: i64) -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    #[tokio::test]
    async fn keeps_create_revision_and_discovery_behind_one_owner_port() {
        let definition = AutomationDefinitionV1::parse_acl(SCHEDULE).expect("definition");
        let revision = AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x400),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision");
        let head =
            AutomationDefinitionV1::from_spec(revision.spec().definition.clone()).expect("head");
        let service = AutomationDefinitionCatalogService::new(Arc::new(
            InMemoryAutomationDefinitionRepository::new(),
        ));
        let created = service
            .create(CreateAutomationDefinition {
                definition: head,
                revision: revision.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(service.list(10).await.expect("list").len(), 1);
        assert_eq!(
            service
                .get(
                    created.definition.spec().organization_id,
                    created.definition.spec().automation_id,
                )
                .await
                .expect("get")
                .expect("record")
                .revision,
            revision
        );
    }
}
