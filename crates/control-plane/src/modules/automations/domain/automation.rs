use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{AutomationDefinitionV1, AutomationRevisionV1};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// The immutable Automations definition head and its exact current revision.
///
/// The definition semantic value is derived from the current revision. The
/// repository persists the canonical revision ACL and digest rather than a
/// second mutable copy of the trigger policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationDefinitionRecord {
    pub definition: AutomationDefinitionV1,
    pub revision: AutomationRevisionV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AutomationDefinitionRecord {
    pub fn new(
        definition: AutomationDefinitionV1,
        revision: AutomationRevisionV1,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let record = Self {
            definition,
            revision,
            created_at,
            updated_at: created_at,
        };
        record.validate_for_creation()?;
        Ok(record)
    }

    pub fn validate_for_creation(&self) -> Result<(), String> {
        self.definition.validate()?;
        self.revision.validate()?;
        if self.revision.spec().revision_number != 1
            || self.revision.spec().parent_revision_id.is_some()
            || self.revision.spec().parent_digest.is_some()
        {
            return Err("initial Automation revision must not have a parent".into());
        }
        if self.revision.spec().definition != *self.definition.spec() {
            return Err("Automation revision definition does not match its head".into());
        }
        validate_identity_scope(&self.revision, self.definition.spec().automation_id)?;
        validate_timestamps(self.created_at, self.updated_at)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate()?;
        if self.revision.spec().definition != *self.definition.spec() {
            return Err("Automation revision definition drifted from its head".into());
        }
        validate_identity_scope(&self.revision, self.definition.spec().automation_id)?;
        validate_timestamps(self.created_at, self.updated_at)
    }

    pub fn append(
        &self,
        revision: AutomationRevisionV1,
        expected_revision_digest: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.revision.digest() != expected_revision_digest {
            return Err("Automation definition head is stale".into());
        }
        revision.validate_successor_of(&self.revision)?;
        if revision.spec().definition.automation_id != self.definition.spec().automation_id
            || revision.spec().definition.organization_id != self.definition.spec().organization_id
            || revision.spec().definition.project_id != self.definition.spec().project_id
            || revision.spec().definition.environment_id != self.definition.spec().environment_id
        {
            return Err("Automation successor changed its immutable scope".into());
        }
        if updated_at < self.updated_at {
            return Err("Automation definition update timestamp precedes its head".into());
        }
        let definition = AutomationDefinitionV1::from_spec(revision.spec().definition.clone())?;
        Ok(Self {
            definition,
            revision,
            created_at: self.created_at,
            updated_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAutomationDefinition {
    pub definition: AutomationDefinitionV1,
    pub revision: AutomationRevisionV1,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendAutomationRevision {
    pub organization_id: Uuid,
    pub automation_id: Uuid,
    pub expected_revision_digest: String,
    pub revision: AutomationRevisionV1,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait IAutomationDefinitionRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateAutomationDefinition,
    ) -> Result<AutomationDefinitionRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        automation_id: Uuid,
    ) -> Result<Option<AutomationDefinitionRecord>, RepositoryError>;

    /// Return a deterministic bounded snapshot of definition heads.
    ///
    /// The result is an owner-facing discovery surface: callers must still
    /// authorize each exact revision and decide which trigger type they own.
    async fn list(&self, limit: usize) -> Result<Vec<AutomationDefinitionRecord>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: Uuid,
        automation_id: Uuid,
        revision_id: Uuid,
    ) -> Result<Option<AutomationRevisionV1>, RepositoryError>;

    async fn append_revision(
        &self,
        request: AppendAutomationRevision,
    ) -> Result<AutomationDefinitionRecord, RepositoryError>;
}

fn validate_identity_scope(
    revision: &AutomationRevisionV1,
    automation_id: Uuid,
) -> Result<(), String> {
    let definition = &revision.spec().definition;
    if definition.automation_id != automation_id
        || definition.automation_id.is_nil()
        || definition.organization_id.is_nil()
        || definition.project_id.is_nil()
        || definition.environment_id.is_nil()
    {
        return Err("Automation definition identity or scope is invalid".into());
    }
    Ok(())
}

fn validate_timestamps(created_at: DateTime<Utc>, updated_at: DateTime<Utc>) -> Result<(), String> {
    if created_at.timestamp_subsec_nanos() != 0 || updated_at.timestamp_subsec_nanos() != 0 {
        return Err("Automation definition timestamps must use whole seconds".into());
    }
    if updated_at < created_at {
        return Err("Automation definition update timestamp precedes creation".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::AutomationDefinitionV1;
    use chrono::TimeZone;

    const DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    fn revision() -> AutomationRevisionV1 {
        let definition = AutomationDefinitionV1::parse_acl(DEFINITION).expect("definition");
        AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x100),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision")
    }

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("timestamp")
    }

    #[test]
    fn creation_requires_an_exact_initial_revision() {
        let revision = revision();
        let definition = AutomationDefinitionV1::from_spec(revision.spec().definition.clone())
            .expect("definition");
        let record = AutomationDefinitionRecord::new(definition, revision, timestamp(1_000))
            .expect("record");
        assert_eq!(record.revision.spec().revision_number, 1);
    }

    #[test]
    fn successor_replaces_only_the_current_head() {
        let first = revision();
        let definition =
            AutomationDefinitionV1::from_spec(first.spec().definition.clone()).expect("definition");
        let record = AutomationDefinitionRecord::new(definition, first.clone(), timestamp(1_000))
            .expect("record");
        let mut spec = first.spec().definition.clone();
        spec.name = "updated-automation".into();
        let second =
            AutomationRevisionV1::from_definition(Uuid::from_u128(0x101), 2, Some(&first), spec)
                .expect("successor");
        let updated = record
            .append(second.clone(), first.digest(), timestamp(1_001))
            .expect("append");
        assert_eq!(updated.revision, second);
        assert_eq!(updated.updated_at, timestamp(1_001));
        assert!(record
            .append(second, "sha256:bad", timestamp(1_001))
            .is_err());
    }
}
