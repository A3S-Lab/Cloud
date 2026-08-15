use super::DurableCellApplicationDefinition;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    OrganizationId, PrincipalId, ProjectId, ResourceName, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableCellApplicationDesiredState {
    Running,
    Stopped,
}

impl DurableCellApplicationDesiredState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            _ => Err(format!(
                "unsupported Durable Cell application desired state {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellApplicationRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub id: DurableCellApplicationRevisionId,
    pub revision_number: u64,
    pub parent_revision_id: Option<DurableCellApplicationRevisionId>,
    pub parent_definition_digest: Option<Sha256Digest>,
    pub definition: DurableCellApplicationDefinition,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl DurableCellApplicationRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn initial(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        id: DurableCellApplicationRevisionId,
        definition: DurableCellApplicationDefinition,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let revision = Self {
            organization_id,
            project_id,
            environment_id,
            application_id,
            id,
            revision_number: 1,
            parent_revision_id: None,
            parent_definition_digest: None,
            definition,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        revision.validate()?;
        Ok(revision)
    }

    pub fn successor(
        parent: &Self,
        id: DurableCellApplicationRevisionId,
        definition: DurableCellApplicationDefinition,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        parent.validate()?;
        definition.validate_successor_of(&parent.definition)?;
        let created_at = canonical_timestamp(created_at);
        if created_at < parent.created_at {
            return Err("Durable Cell revision time must not precede its parent".into());
        }
        let revision = Self {
            organization_id: parent.organization_id,
            project_id: parent.project_id,
            environment_id: parent.environment_id,
            application_id: parent.application_id,
            id,
            revision_number: parent
                .revision_number
                .checked_add(1)
                .ok_or_else(|| "Durable Cell revision number is exhausted".to_owned())?,
            parent_revision_id: Some(parent.id),
            parent_definition_digest: Some(parent.definition.digest().clone()),
            definition,
            created_by,
            created_at,
        };
        revision.validate()?;
        Ok(revision)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.definition = DurableCellApplicationDefinition::restore(
            self.definition.canonical_acl(),
            self.definition.digest().as_str(),
        )?;
        self.created_at = canonical_timestamp(self.created_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.revision_number == 0
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Durable Cell revision identity or timestamp is invalid".into());
        }
        self.definition.validate()?;
        match (&self.parent_revision_id, &self.parent_definition_digest) {
            (None, None) if self.revision_number == 1 => Ok(()),
            (Some(parent_id), Some(parent_digest))
                if self.revision_number > 1
                    && !parent_id.as_uuid().is_nil()
                    && parent_digest != self.definition.digest() =>
            {
                Ok(())
            }
            _ => Err("Durable Cell revision lineage is invalid".into()),
        }
    }
}

/// Tenant application intent. Runtime Service replicas and provider state are
/// projections of this aggregate, never fields inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: DurableCellApplicationId,
    pub name: ResourceName,
    pub desired_state: DurableCellApplicationDesiredState,
    pub current_revision_id: DurableCellApplicationRevisionId,
    pub current_revision_number: u64,
    pub current_definition_digest: Sha256Digest,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DurableCellApplication {
    pub fn create(
        id: DurableCellApplicationId,
        name: ResourceName,
        revision: &DurableCellApplicationRevision,
    ) -> Result<Self, String> {
        revision.validate()?;
        if id != revision.application_id || revision.revision_number != 1 {
            return Err("initial Durable Cell revision does not belong to the application".into());
        }
        let application = Self {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            id,
            name,
            desired_state: DurableCellApplicationDesiredState::Running,
            current_revision_id: revision.id,
            current_revision_number: 1,
            current_definition_digest: revision.definition.digest().clone(),
            aggregate_version: 1,
            created_by: revision.created_by,
            created_at: revision.created_at,
            updated_at: revision.created_at,
        };
        application.validate()?;
        Ok(application)
    }

    pub fn advance(
        &self,
        expected_aggregate_version: u64,
        revision: &DurableCellApplicationRevision,
    ) -> Result<Self, String> {
        self.validate()?;
        revision.validate()?;
        if expected_aggregate_version == 0
            || self.aggregate_version != expected_aggregate_version
            || revision.organization_id != self.organization_id
            || revision.project_id != self.project_id
            || revision.environment_id != self.environment_id
            || revision.application_id != self.id
            || revision.revision_number != self.current_revision_number.saturating_add(1)
            || revision.parent_revision_id != Some(self.current_revision_id)
            || revision.parent_definition_digest.as_ref() != Some(&self.current_definition_digest)
            || revision.created_at < self.updated_at
        {
            return Err("Durable Cell application was revised from stale or foreign state".into());
        }
        let aggregate_version = expected_aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Durable Cell aggregate version is exhausted".to_owned())?;
        let application = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            id: self.id,
            name: self.name.clone(),
            desired_state: self.desired_state,
            current_revision_id: revision.id,
            current_revision_number: revision.revision_number,
            current_definition_digest: revision.definition.digest().clone(),
            aggregate_version,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: revision.created_at,
        };
        application.validate()?;
        Ok(application)
    }

    pub fn request_state(
        &self,
        expected_aggregate_version: u64,
        desired_state: DurableCellApplicationDesiredState,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        let requested_at = canonical_timestamp(requested_at);
        if expected_aggregate_version == 0
            || self.aggregate_version != expected_aggregate_version
            || requested_at < self.updated_at
        {
            return Err("Durable Cell desired-state request is stale".into());
        }
        if self.desired_state == desired_state {
            return Ok(self.clone());
        }
        let mut application = self.clone();
        application.desired_state = desired_state;
        application.aggregate_version = expected_aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Durable Cell aggregate version is exhausted".to_owned())?;
        application.updated_at = requested_at;
        application.validate()?;
        Ok(application)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.created_at = canonical_timestamp(self.created_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        let canonical_name = ResourceName::parse(self.name.as_str().to_owned())?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.current_revision_id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.current_revision_number == 0
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || canonical_name != self.name
            || Sha256Digest::parse(self.current_definition_digest.as_str())?
                != self.current_definition_digest
        {
            return Err("stored Durable Cell application is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::durable_cells::domain::{
        DurableCellApplicationDefinitionSpec, DurableCellClassSpec, DurableCellRollbackPolicy,
        DurableCellStateSchema,
    };
    use crate::modules::shared_kernel::domain::BuildRunId;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn definition(character: char, write_version: u64) -> DurableCellApplicationDefinition {
        DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
            build_run_id: BuildRunId::new(),
            bundle_digest: digest(character),
            bundle_size_bytes: 1024,
            main_module: "worker.mjs".into(),
            compatibility_date: "2026-08-15".into(),
            compatibility_flags: Vec::new(),
            cell_classes: vec![DurableCellClassSpec {
                name: "Counter".into(),
                state_schema: DurableCellStateSchema {
                    minimum_readable_version: 1,
                    maximum_readable_version: 2,
                    write_version,
                },
            }],
            service_profile_digest: digest('f'),
            rollback_policy: DurableCellRollbackPolicy::Compatible,
        })
        .expect("definition")
    }

    #[test]
    fn immutable_revision_and_application_advance_only_through_exact_lineage() {
        let application_id = DurableCellApplicationId::new();
        let initial = DurableCellApplicationRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition('a', 1),
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("initial revision");
        let application = DurableCellApplication::create(
            application_id,
            ResourceName::parse("Tenant Counters").expect("name"),
            &initial,
        )
        .expect("application");
        let stopped = application
            .request_state(
                1,
                DurableCellApplicationDesiredState::Stopped,
                initial.created_at,
            )
            .expect("stop");
        let successor = DurableCellApplicationRevision::successor(
            &initial,
            DurableCellApplicationRevisionId::new(),
            definition('b', 2),
            PrincipalId::new(),
            initial.created_at,
        )
        .expect("successor");
        let advanced = stopped
            .advance(2, &successor)
            .expect("advance while stopped");
        assert_eq!(advanced.current_revision_number, 2);
        assert_eq!(advanced.aggregate_version, 3);
        assert_eq!(
            advanced.desired_state,
            DurableCellApplicationDesiredState::Stopped
        );
        let running = advanced
            .request_state(
                3,
                DurableCellApplicationDesiredState::Running,
                successor.created_at,
            )
            .expect("restart");
        assert_eq!(running.aggregate_version, 4);
        assert!(stopped.advance(1, &successor).is_err());
    }

    #[test]
    fn revision_rejects_noop_or_incompatible_successors() {
        let application_id = DurableCellApplicationId::new();
        let initial = DurableCellApplicationRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition('a', 1),
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("initial revision");
        assert!(DurableCellApplicationRevision::successor(
            &initial,
            DurableCellApplicationRevisionId::new(),
            initial.definition.clone(),
            PrincipalId::new(),
            initial.created_at,
        )
        .is_err());

        let mut incompatible = definition('b', 2).spec().clone();
        incompatible.cell_classes[0]
            .state_schema
            .minimum_readable_version = 2;
        assert!(DurableCellApplicationRevision::successor(
            &initial,
            DurableCellApplicationRevisionId::new(),
            DurableCellApplicationDefinition::from_spec(incompatible).expect("definition"),
            PrincipalId::new(),
            initial.created_at,
        )
        .is_err());
    }
}
