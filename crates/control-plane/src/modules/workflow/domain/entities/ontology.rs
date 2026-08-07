use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
    Sha256Digest,
};
use crate::modules::workflow::domain::value_objects::OntologyName;
use crate::modules::workflow::domain::OntologyRevision;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ontology {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: OntologyId,
    pub name: OntologyName,
    pub description: String,
    pub current_revision_id: OntologyRevisionId,
    pub current_revision_number: u64,
    pub current_revision_digest: Sha256Digest,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Ontology {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: OntologyId,
        name: OntologyName,
        description: String,
        revision_id: OntologyRevisionId,
        revision_digest: Sha256Digest,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_description(&description)?;
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
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
        })
    }

    pub fn advance(
        &self,
        expected_version: u64,
        name: OntologyName,
        description: String,
        revision_id: OntologyRevisionId,
        revision_digest: Sha256Digest,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if expected_version == 0 || self.aggregate_version != expected_version {
            return Err("Ontology aggregate version does not match the expected version".into());
        }
        validate_description(&description)?;
        let updated_at = canonical_timestamp(updated_at);
        if updated_at < self.updated_at {
            return Err("Ontology update time must not precede the current revision".into());
        }
        let next_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Ontology aggregate version is exhausted".to_owned())?;
        Ok(Self {
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
        })
    }

    pub fn at_revision(&self, revision: &OntologyRevision) -> Result<Self, String> {
        if revision.organization_id != self.organization_id
            || revision.project_id != self.project_id
            || revision.ontology_id != self.id
            || revision.created_at < self.created_at
        {
            return Err("Ontology revision does not belong to this aggregate".into());
        }
        let ontology = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            name: OntologyName::parse(revision.contract.spec().name.clone())?,
            description: revision.contract.spec().description.clone(),
            current_revision_id: revision.id,
            current_revision_number: revision.revision_number,
            current_revision_digest: revision.contract.digest().clone(),
            aggregate_version: revision.revision_number,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: revision.created_at,
        };
        ontology.validate()?;
        Ok(ontology)
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
            return Err("stored Ontology aggregate is invalid".into());
        }
        validate_description(&self.description)
    }
}

fn validate_description(value: &str) -> Result<(), String> {
    if value.chars().count() > 4_096 || value.contains('\0') {
        return Err("Ontology description must contain at most 4096 safe characters".into());
    }
    Ok(())
}
