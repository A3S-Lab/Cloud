use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::WorkflowRevision;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: WorkflowDefinitionId,
    pub name: String,
    pub description: String,
    pub current_revision_id: WorkflowRevisionId,
    pub current_revision_number: u64,
    pub current_revision_digest: Sha256Digest,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkflowDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: WorkflowDefinitionId,
        name: String,
        description: String,
        revision_id: WorkflowRevisionId,
        revision_digest: Sha256Digest,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_identity_text(&name, &description)?;
        let created_at = canonical_timestamp(created_at);
        let value = Self {
            organization_id,
            project_id,
            id,
            name,
            description,
            current_revision_id: revision_id,
            current_revision_number: 1,
            current_revision_digest: revision_digest,
            aggregate_version: 1,
            created_by,
            created_at,
            updated_at: created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn advance(
        &self,
        expected_version: u64,
        name: String,
        description: String,
        revision_id: WorkflowRevisionId,
        revision_digest: Sha256Digest,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if expected_version == 0 || self.aggregate_version != expected_version {
            return Err(
                "WorkflowDefinition aggregate version does not match the expected version".into(),
            );
        }
        validate_identity_text(&name, &description)?;
        let updated_at = canonical_timestamp(updated_at);
        if updated_at < self.updated_at {
            return Err("WorkflowDefinition update time precedes the current revision".into());
        }
        let next_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "WorkflowDefinition aggregate version is exhausted".to_owned())?;
        let value = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            name,
            description,
            current_revision_id: revision_id,
            current_revision_number: next_version,
            current_revision_digest: revision_digest,
            aggregate_version: next_version,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn at_revision(&self, revision: &WorkflowRevision) -> Result<Self, String> {
        if revision.organization_id != self.organization_id
            || revision.project_id != self.project_id
            || revision.workflow_definition_id != self.id
            || revision.created_at < self.created_at
        {
            return Err("Workflow revision does not belong to this definition".into());
        }
        let value = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            name: revision.contract.spec().name.clone(),
            description: revision.contract.spec().description.clone(),
            current_revision_id: revision.id,
            current_revision_number: revision.revision_number,
            current_revision_digest: revision.contract.digest().clone(),
            aggregate_version: revision.revision_number,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: revision.created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.current_revision_id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.current_revision_number != self.aggregate_version
            || self.updated_at < self.created_at
        {
            return Err("stored WorkflowDefinition aggregate is invalid".into());
        }
        validate_identity_text(&self.name, &self.description)
    }
}

fn validate_identity_text(name: &str, description: &str) -> Result<(), String> {
    super::super::validation::validate_text("Workflow name", name, 1, 120)?;
    super::super::validation::validate_text("Workflow description", description, 0, 4_096)
}
