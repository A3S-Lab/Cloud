use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
    Sha256Digest,
};
use crate::modules::workflow::domain::value_objects::OntologyMigrationPolicy;
use crate::modules::workflow::domain::{OntologyContract, ONTOLOGY_SCHEMA};
use chrono::{DateTime, Utc};

pub const ONTOLOGY_COMPILER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub ontology_id: OntologyId,
    pub id: OntologyRevisionId,
    pub revision_number: u64,
    pub parent_revision_id: Option<OntologyRevisionId>,
    pub parent_digest: Option<Sha256Digest>,
    pub contract: OntologyContract,
    pub compiler_schema_version: u32,
    pub migration_policy: OntologyMigrationPolicy,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl OntologyRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn initial(
        organization_id: OrganizationId,
        project_id: ProjectId,
        ontology_id: OntologyId,
        id: OntologyRevisionId,
        contract: OntologyContract,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            ontology_id,
            id,
            revision_number: 1,
            parent_revision_id: None,
            parent_digest: None,
            contract,
            compiler_schema_version: ONTOLOGY_COMPILER_SCHEMA_VERSION,
            migration_policy: OntologyMigrationPolicy::Initial,
            created_by,
            created_at: canonical_timestamp(created_at),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn successor(
        parent: &Self,
        id: OntologyRevisionId,
        contract: OntologyContract,
        migration_policy: OntologyMigrationPolicy,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if matches!(migration_policy, OntologyMigrationPolicy::Initial) {
            return Err(
                "successor Ontology revision cannot use the initial migration policy".into(),
            );
        }
        let revision_number = parent
            .revision_number
            .checked_add(1)
            .ok_or_else(|| "Ontology revision number is exhausted".to_owned())?;
        Ok(Self {
            organization_id: parent.organization_id,
            project_id: parent.project_id,
            ontology_id: parent.ontology_id,
            id,
            revision_number,
            parent_revision_id: Some(parent.id),
            parent_digest: Some(parent.contract.digest().clone()),
            contract,
            compiler_schema_version: ONTOLOGY_COMPILER_SCHEMA_VERSION,
            migration_policy,
            created_by,
            created_at: canonical_timestamp(created_at),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        ontology_id: OntologyId,
        id: OntologyRevisionId,
        revision_number: u64,
        parent_revision_id: Option<OntologyRevisionId>,
        parent_digest: Option<Sha256Digest>,
        acl: &str,
        stored_digest: &str,
        compiler_schema_version: u32,
        migration_policy: OntologyMigrationPolicy,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let revision = Self {
            organization_id,
            project_id,
            ontology_id,
            id,
            revision_number,
            parent_revision_id,
            parent_digest,
            contract: OntologyContract::restore(acl, stored_digest)?,
            compiler_schema_version,
            migration_policy,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        revision.validate()?;
        Ok(revision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.ontology_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.revision_number == 0
            || self.compiler_schema_version != ONTOLOGY_COMPILER_SCHEMA_VERSION
        {
            return Err("stored Ontology revision is invalid".into());
        }
        match (
            &self.parent_revision_id,
            &self.parent_digest,
            &self.migration_policy,
        ) {
            (None, None, OntologyMigrationPolicy::Initial) if self.revision_number == 1 => Ok(()),
            (Some(parent_id), Some(_), OntologyMigrationPolicy::Compatible)
            | (Some(parent_id), Some(_), OntologyMigrationPolicy::Explicit { .. })
                if self.revision_number > 1 && !parent_id.as_uuid().is_nil() =>
            {
                Ok(())
            }
            _ => Err("Ontology revision lineage or migration policy is invalid".into()),
        }
    }

    pub const fn contract_schema(&self) -> &'static str {
        ONTOLOGY_SCHEMA
    }
}
