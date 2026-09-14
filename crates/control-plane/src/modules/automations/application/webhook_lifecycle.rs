use super::resource_access::{
    definition_not_found, endpoint_not_found, environment, revision_not_found, AutomationAccess,
};
use crate::modules::automations::domain::{
    AutomationWebhookEndpointRecord, EndpointLifecycleAction, IAutomationDefinitionRepository,
    IAutomationWebhookRepository, TransitionAutomationWebhookEndpoint,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_cloud_contracts::{
    AutomationTriggerV1, AutomationWebhookSecretReferenceV1,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

/// Authorized create for one Automation webhook endpoint over an exact revision.
#[derive(Debug, Clone)]
pub struct CreateAuthorizedAutomationWebhookEndpoint {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub endpoint_id: Uuid,
    pub endpoint_key: String,
    pub signing_secret: AutomationWebhookSecretReferenceV1,
    pub max_body_bytes: u64,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub access: AutomationAccess,
    pub created_at: DateTime<Utc>,
}

/// Authorized get for one Automation webhook endpoint.
#[derive(Debug, Clone)]
pub struct GetAuthorizedAutomationWebhookEndpoint {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub endpoint_id: Uuid,
    pub access: AutomationAccess,
}

/// Authorized generation-fenced lifecycle transition.
#[derive(Debug, Clone)]
pub struct ChangeAuthorizedAutomationWebhookEndpoint {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub endpoint_id: Uuid,
    pub expected_generation: u64,
    pub action: EndpointLifecycleAction,
    pub access: AutomationAccess,
    pub changed_at: DateTime<Utc>,
}

/// Authorized webhook endpoint lifecycle over the existing C2/C3 authorities.
///
/// Authorization precedes every repository call. This boundary does not add a
/// second persistence, Gateway receive route, or schema-registry authority.
#[derive(Clone)]
pub struct AutomationWebhookLifecycleService {
    webhooks: Arc<dyn IAutomationWebhookRepository>,
    definitions: Arc<dyn IAutomationDefinitionRepository>,
}

impl AutomationWebhookLifecycleService {
    pub fn new(
        webhooks: Arc<dyn IAutomationWebhookRepository>,
        definitions: Arc<dyn IAutomationDefinitionRepository>,
    ) -> Self {
        Self {
            webhooks,
            definitions,
        }
    }

    pub async fn create_endpoint(
        &self,
        command: CreateAuthorizedAutomationWebhookEndpoint,
    ) -> ApplicationResult<AutomationWebhookEndpointRecord> {
        environment(command.project_id, command.environment_id, &command.access)?;
        let revision = self
            .definitions
            .find_revision(
                command.organization_id.as_uuid(),
                command.automation_id,
                command.revision_id,
            )
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(revision_not_found)?;
        let definition = revision.spec().definition.clone();
        if definition.organization_id != command.organization_id.as_uuid()
            || definition.project_id != command.project_id.as_uuid()
            || definition.environment_id != command.environment_id.as_uuid()
            || definition.automation_id != command.automation_id
        {
            return Err(ApplicationError::Invalid(
                "Automation revision is outside the requested webhook endpoint scope".into(),
            ));
        }
        if !matches!(definition.trigger, AutomationTriggerV1::Webhook(_)) {
            return Err(ApplicationError::Invalid(
                "Automation webhook endpoints require an exact webhook-trigger revision".into(),
            ));
        }
        let endpoint = a3s_cloud_contracts::AutomationWebhookEndpointV1::for_revision(
            command.endpoint_id,
            command.endpoint_key,
            command.signing_secret,
            command.max_body_bytes,
            &revision,
            command.created_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let record = AutomationWebhookEndpointRecord::new(endpoint, revision)
            .map_err(ApplicationError::Invalid)?;
        self.webhooks
            .create_endpoint(record)
            .await
            .map_err(Into::into)
    }

    pub async fn get_endpoint(
        &self,
        query: GetAuthorizedAutomationWebhookEndpoint,
    ) -> ApplicationResult<AutomationWebhookEndpointRecord> {
        environment(query.project_id, query.environment_id, &query.access)?;
        let record = self
            .webhooks
            .find_endpoint(query.endpoint_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(endpoint_not_found)?;
        if record.endpoint.organization_id != query.organization_id.as_uuid()
            || record.endpoint.project_id != query.project_id.as_uuid()
            || record.endpoint.environment_id != query.environment_id.as_uuid()
        {
            return Err(endpoint_not_found());
        }
        Ok(record)
    }

    pub async fn change_endpoint(
        &self,
        command: ChangeAuthorizedAutomationWebhookEndpoint,
    ) -> ApplicationResult<AutomationWebhookEndpointRecord> {
        // Authorize against the durable endpoint scope before generation CAS.
        let _current = self
            .get_endpoint(GetAuthorizedAutomationWebhookEndpoint {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                endpoint_id: command.endpoint_id,
                access: command.access,
            })
            .await?;
        self.webhooks
            .transition_endpoint(TransitionAutomationWebhookEndpoint {
                endpoint_id: command.endpoint_id,
                expected_generation: command.expected_generation,
                action: command.action,
                changed_at: command.changed_at,
            })
            .await
            .map_err(Into::into)
    }
}

/// Authorized definition/revision catalog reads over the existing C3 catalog.
#[derive(Debug, Clone)]
pub struct GetAuthorizedAutomationDefinition {
    pub organization_id: OrganizationId,
    pub automation_id: Uuid,
    pub access: AutomationAccess,
}

#[derive(Debug, Clone)]
pub struct ListAuthorizedAutomationDefinitions {
    pub organization_id: OrganizationId,
    pub limit: Option<usize>,
    pub access: AutomationAccess,
}

#[derive(Debug, Clone)]
pub struct GetAuthorizedAutomationRevision {
    pub organization_id: OrganizationId,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub access: AutomationAccess,
}

pub const DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT: usize = 50;
pub const MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT: usize = 200;

#[derive(Clone)]
pub struct AutomationDefinitionQueryService {
    definitions: Arc<dyn IAutomationDefinitionRepository>,
}

impl AutomationDefinitionQueryService {
    pub fn new(definitions: Arc<dyn IAutomationDefinitionRepository>) -> Self {
        Self { definitions }
    }

    pub async fn get(
        &self,
        query: GetAuthorizedAutomationDefinition,
    ) -> ApplicationResult<crate::modules::automations::domain::AutomationDefinitionRecord> {
        let record = self
            .definitions
            .find(query.organization_id.as_uuid(), query.automation_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(definition_not_found)?;
        let spec = record.definition.spec();
        environment(
            ProjectId::from_uuid(spec.project_id),
            EnvironmentId::from_uuid(spec.environment_id),
            &query.access,
        )?;
        if spec.organization_id != query.organization_id.as_uuid() {
            return Err(definition_not_found());
        }
        Ok(record)
    }

    pub async fn list(
        &self,
        query: ListAuthorizedAutomationDefinitions,
    ) -> ApplicationResult<Vec<crate::modules::automations::domain::AutomationDefinitionRecord>>
    {
        let limit = query
            .limit
            .unwrap_or(DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT);
        if limit == 0 || limit > MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT {
            return Err(ApplicationError::Invalid(format!(
                "Automation definition list limit must be 1 through {MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT}"
            )));
        }
        // Discovery is bounded; authorize each visible head after the owner port returns.
        let candidates = self
            .definitions
            .list(MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT)
            .await
            .map_err(ApplicationError::from)?;
        Ok(candidates
            .into_iter()
            .filter(|record| {
                let spec = record.definition.spec();
                spec.organization_id == query.organization_id.as_uuid()
                    && query.access.environment_is_visible(
                        ProjectId::from_uuid(spec.project_id),
                        EnvironmentId::from_uuid(spec.environment_id),
                    )
            })
            .take(limit)
            .collect())
    }

    pub async fn get_revision(
        &self,
        query: GetAuthorizedAutomationRevision,
    ) -> ApplicationResult<a3s_cloud_contracts::AutomationRevisionV1> {
        // Authorize through the head first so tenants cannot probe foreign revisions.
        let _head = self
            .get(GetAuthorizedAutomationDefinition {
                organization_id: query.organization_id,
                automation_id: query.automation_id,
                access: query.access,
            })
            .await?;
        self.definitions
            .find_revision(
                query.organization_id.as_uuid(),
                query.automation_id,
                query.revision_id,
            )
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(revision_not_found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::automations::infrastructure::{
        InMemoryAutomationDefinitionRepository, InMemoryAutomationWebhookRepository,
    };
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationRevisionV1, AutomationWebhookSecretReferenceV1,
    };
    use chrono::DateTime;

    const WEBHOOK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));
    const SCHEDULE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-schedule.acl"
    ));

    fn ts(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    fn ids() -> (OrganizationId, ProjectId, EnvironmentId, Uuid, Uuid) {
        (
            OrganizationId::from_uuid(Uuid::parse_str("018f0000-0000-7000-8000-000000000201").unwrap()),
            ProjectId::from_uuid(Uuid::parse_str("018f0000-0000-7000-8000-000000000202").unwrap()),
            EnvironmentId::from_uuid(Uuid::parse_str("018f0000-0000-7000-8000-000000000404").unwrap()),
            Uuid::parse_str("018f0000-0000-7000-8000-000000000401").unwrap(),
            Uuid::from_u128(0x018f0000000070008000000000000408),
        )
    }

    async fn seed_webhook_definition(
        definitions: &InMemoryAutomationDefinitionRepository,
    ) -> AutomationRevisionV1 {
        let definition = AutomationDefinitionV1::parse_acl(WEBHOOK).expect("definition");
        let (organization_id, _, _, automation_id, revision_id) = ids();
        let revision = AutomationRevisionV1::from_definition(
            revision_id,
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision");
        definitions
            .create(crate::modules::automations::domain::CreateAutomationDefinition {
                definition,
                revision: revision.clone(),
                created_at: ts(1_000),
            })
            .await
            .expect("create definition");
        assert_eq!(organization_id.as_uuid(), revision.spec().definition.organization_id);
        assert_eq!(automation_id, revision.spec().definition.automation_id);
        revision
    }

    #[tokio::test]
    async fn create_get_and_lifecycle_require_environment_access() {
        let definitions = Arc::new(InMemoryAutomationDefinitionRepository::default());
        let webhooks = Arc::new(InMemoryAutomationWebhookRepository::default());
        let revision = seed_webhook_definition(&definitions).await;
        let lifecycle = AutomationWebhookLifecycleService::new(webhooks, definitions);
        let (organization_id, project_id, environment_id, automation_id, revision_id) = ids();
        let denied = lifecycle
            .create_endpoint(CreateAuthorizedAutomationWebhookEndpoint {
                organization_id,
                project_id,
                environment_id,
                endpoint_id: Uuid::from_u128(0x018f0000000070008000000000000409),
                endpoint_key: "release-hook".into(),
                signing_secret: AutomationWebhookSecretReferenceV1 {
                    secret_id: Uuid::from_u128(0x018f000000007000800000000000040a),
                    version: 4,
                },
                max_body_bytes: 4_096,
                automation_id,
                revision_id,
                access: AutomationAccess::restricted([]),
                created_at: ts(1_010),
            })
            .await;
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));

        let created = lifecycle
            .create_endpoint(CreateAuthorizedAutomationWebhookEndpoint {
                organization_id,
                project_id,
                environment_id,
                endpoint_id: Uuid::from_u128(0x018f0000000070008000000000000409),
                endpoint_key: "release-hook".into(),
                signing_secret: AutomationWebhookSecretReferenceV1 {
                    secret_id: Uuid::from_u128(0x018f000000007000800000000000040a),
                    version: 4,
                },
                max_body_bytes: 4_096,
                automation_id,
                revision_id,
                access: AutomationAccess::organization_wide(),
                created_at: ts(1_010),
            })
            .await
            .expect("create");
        assert_eq!(created.revision.digest(), revision.digest());

        let got = lifecycle
            .get_endpoint(GetAuthorizedAutomationWebhookEndpoint {
                organization_id,
                project_id,
                environment_id,
                endpoint_id: created.endpoint.endpoint_id,
                access: AutomationAccess::organization_wide(),
            })
            .await
            .expect("get");
        assert_eq!(got.endpoint.endpoint_key, "release-hook");

        let disabled = lifecycle
            .change_endpoint(ChangeAuthorizedAutomationWebhookEndpoint {
                organization_id,
                project_id,
                environment_id,
                endpoint_id: created.endpoint.endpoint_id,
                expected_generation: created.endpoint.generation,
                action: EndpointLifecycleAction::Disable,
                access: AutomationAccess::organization_wide(),
                changed_at: ts(1_020),
            })
            .await
            .expect("disable");
        assert!(!disabled.endpoint.state.is_accepting());
    }

    #[tokio::test]
    async fn rejects_non_webhook_revision() {
        let definitions = Arc::new(InMemoryAutomationDefinitionRepository::default());
        let webhooks = Arc::new(InMemoryAutomationWebhookRepository::default());
        let definition = AutomationDefinitionV1::parse_acl(SCHEDULE).expect("schedule");
        let spec = definition.spec().clone();
        let revision_id = Uuid::from_u128(0x018f0000000070008000000000000508);
        let revision = AutomationRevisionV1::from_definition(
            revision_id,
            1,
            None,
            spec.clone(),
        )
        .expect("revision");
        definitions
            .create(crate::modules::automations::domain::CreateAutomationDefinition {
                definition,
                revision,
                created_at: ts(1_000),
            })
            .await
            .expect("create");
        let lifecycle = AutomationWebhookLifecycleService::new(webhooks, definitions);
        let err = lifecycle
            .create_endpoint(CreateAuthorizedAutomationWebhookEndpoint {
                organization_id: OrganizationId::from_uuid(spec.organization_id),
                project_id: ProjectId::from_uuid(spec.project_id),
                environment_id: EnvironmentId::from_uuid(spec.environment_id),
                endpoint_id: Uuid::from_u128(0x1),
                endpoint_key: "nope".into(),
                signing_secret: AutomationWebhookSecretReferenceV1 {
                    secret_id: Uuid::from_u128(0x2),
                    version: 1,
                },
                max_body_bytes: 1024,
                automation_id: spec.automation_id,
                revision_id,
                access: AutomationAccess::organization_wide(),
                created_at: ts(1_010),
            })
            .await;
        assert!(matches!(err, Err(ApplicationError::Invalid(_))), "{err:?}");
    }
}

