use super::{ConnectorHttpDefinition, ConnectorSecretBinding, CONNECTOR_HTTP_DEFINITION_SCHEMA};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId,
    PrincipalId, ProjectId, ResourceName, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "definition", rename_all = "snake_case")]
pub enum ConnectorDefinition {
    Http(ConnectorHttpDefinition),
}

impl ConnectorDefinition {
    pub fn parse_acl(source: &str) -> Result<Self, String> {
        ConnectorHttpDefinition::parse_acl(source).map(Self::Http)
    }

    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Http(_) => "http",
        }
    }

    pub const fn schema(&self) -> &'static str {
        match self {
            Self::Http(_) => CONNECTOR_HTTP_DEFINITION_SCHEMA,
        }
    }

    pub fn canonical_acl(&self) -> &str {
        match self {
            Self::Http(definition) => definition.canonical_acl(),
        }
    }

    pub const fn digest(&self) -> &Sha256Digest {
        match self {
            Self::Http(definition) => definition.digest(),
        }
    }

    pub fn secret_bindings(&self) -> Vec<ConnectorSecretBinding> {
        match self {
            Self::Http(definition) => definition.secret_bindings(),
        }
    }

    pub fn restore(
        kind: &str,
        schema: &str,
        canonical_acl: &str,
        stored_digest: &str,
    ) -> Result<Self, String> {
        match (kind, schema) {
            ("http", CONNECTOR_HTTP_DEFINITION_SCHEMA) => Ok(Self::Http(
                ConnectorHttpDefinition::restore(canonical_acl, stored_digest)?,
            )),
            _ => Err("stored Connector definition kind or schema is unsupported".into()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(
            self.kind(),
            self.schema(),
            self.canonical_acl(),
            self.digest().as_str(),
        )?;
        if &restored != self {
            return Err("Connector definition drifted from its canonical ACL".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub id: ConnectorRevisionId,
    pub revision_number: u64,
    pub parent_revision_id: Option<ConnectorRevisionId>,
    pub parent_digest: Option<Sha256Digest>,
    pub definition: ConnectorDefinition,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl ConnectorRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn initial(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        id: ConnectorRevisionId,
        definition: ConnectorDefinition,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let revision = Self {
            organization_id,
            project_id,
            environment_id,
            profile_id,
            id,
            revision_number: 1,
            parent_revision_id: None,
            parent_digest: None,
            definition,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        revision.validate()?;
        Ok(revision)
    }

    pub fn successor(
        parent: &Self,
        id: ConnectorRevisionId,
        definition: ConnectorDefinition,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        parent.validate()?;
        definition.validate()?;
        if definition.digest() == parent.definition.digest() {
            return Err("successor Connector revision must change the definition digest".into());
        }
        let created_at = canonical_timestamp(created_at);
        if created_at < parent.created_at {
            return Err("Connector revision time must not precede its parent".into());
        }
        let revision = Self {
            organization_id: parent.organization_id,
            project_id: parent.project_id,
            environment_id: parent.environment_id,
            profile_id: parent.profile_id,
            id,
            revision_number: parent
                .revision_number
                .checked_add(1)
                .ok_or_else(|| "Connector revision number is exhausted".to_owned())?,
            parent_revision_id: Some(parent.id),
            parent_digest: Some(parent.definition.digest().clone()),
            definition,
            created_by,
            created_at,
        };
        revision.validate()?;
        Ok(revision)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        id: ConnectorRevisionId,
        revision_number: u64,
        parent_revision_id: Option<ConnectorRevisionId>,
        parent_digest: Option<Sha256Digest>,
        definition_kind: &str,
        definition_schema: &str,
        canonical_acl: &str,
        definition_digest: &str,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let revision = Self {
            organization_id,
            project_id,
            environment_id,
            profile_id,
            id,
            revision_number,
            parent_revision_id,
            parent_digest,
            definition: ConnectorDefinition::restore(
                definition_kind,
                definition_schema,
                canonical_acl,
                definition_digest,
            )?,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        revision.validate()?;
        Ok(revision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.profile_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.revision_number == 0
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Connector revision identity or timestamp is invalid".into());
        }
        self.definition.validate()?;
        match (&self.parent_revision_id, &self.parent_digest) {
            (None, None) if self.revision_number == 1 => Ok(()),
            (Some(parent_id), Some(parent_digest))
                if self.revision_number > 1
                    && !parent_id.as_uuid().is_nil()
                    && parent_digest != self.definition.digest() =>
            {
                Ok(())
            }
            _ => Err("Connector revision lineage is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: ConnectorProfileId,
    pub name: ResourceName,
    pub current_revision_id: ConnectorRevisionId,
    pub current_revision_number: u64,
    pub current_revision_digest: Sha256Digest,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectorProfile {
    pub fn create(
        id: ConnectorProfileId,
        name: ResourceName,
        revision: &ConnectorRevision,
    ) -> Result<Self, String> {
        revision.validate()?;
        if id != revision.profile_id || revision.revision_number != 1 {
            return Err("initial Connector revision does not belong to the profile".into());
        }
        let profile = Self {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            id,
            name,
            current_revision_id: revision.id,
            current_revision_number: 1,
            current_revision_digest: revision.definition.digest().clone(),
            aggregate_version: 1,
            created_by: revision.created_by,
            created_at: revision.created_at,
            updated_at: revision.created_at,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn advance(
        &self,
        expected_version: u64,
        revision: &ConnectorRevision,
    ) -> Result<Self, String> {
        self.validate()?;
        revision.validate()?;
        if expected_version == 0
            || self.aggregate_version != expected_version
            || revision.organization_id != self.organization_id
            || revision.project_id != self.project_id
            || revision.environment_id != self.environment_id
            || revision.profile_id != self.id
            || revision.revision_number != expected_version.saturating_add(1)
            || revision.parent_revision_id != Some(self.current_revision_id)
            || revision.parent_digest.as_ref() != Some(&self.current_revision_digest)
            || revision.created_at < self.updated_at
        {
            return Err("Connector profile was revised from a stale or foreign revision".into());
        }
        let aggregate_version = expected_version
            .checked_add(1)
            .ok_or_else(|| "Connector profile aggregate version is exhausted".to_owned())?;
        let profile = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            id: self.id,
            name: self.name.clone(),
            current_revision_id: revision.id,
            current_revision_number: revision.revision_number,
            current_revision_digest: revision.definition.digest().clone(),
            aggregate_version,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: revision.created_at,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn at_revision(&self, revision: &ConnectorRevision) -> Result<Self, String> {
        self.validate()?;
        revision.validate()?;
        if revision.organization_id != self.organization_id
            || revision.project_id != self.project_id
            || revision.environment_id != self.environment_id
            || revision.profile_id != self.id
            || revision.created_at < self.created_at
            || revision.revision_number > self.current_revision_number
        {
            return Err("Connector revision does not belong to this profile".into());
        }
        let profile = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            id: self.id,
            name: self.name.clone(),
            current_revision_id: revision.id,
            current_revision_number: revision.revision_number,
            current_revision_digest: revision.definition.digest().clone(),
            aggregate_version: revision.revision_number,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: revision.created_at,
        };
        profile.validate()?;
        Ok(profile)
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
            || self.current_revision_number != self.aggregate_version
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || canonical_name != self.name
        {
            return Err("stored Connector profile is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorHttpAuthentication, ConnectorHttpDefinitionSpec, ConnectorHttpDestination,
        ConnectorHttpMethod, ConnectorHttpStatusPolicy,
    };
    use crate::modules::shared_kernel::domain::SecretId;

    fn definition(timeout_milliseconds: u64) -> ConnectorDefinition {
        ConnectorDefinition::Http(
            ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                destination: ConnectorHttpDestination::SecretHttpsUrl {
                    reference: super::super::ConnectorSecretReference::new(SecretId::new(), 2)
                        .expect("destination Secret"),
                },
                method: ConnectorHttpMethod::Post,
                request_content_type: "application/json".into(),
                maximum_request_bytes: 16 * 1024,
                maximum_response_bytes: 16 * 1024,
                timeout_milliseconds,
                status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                authentication: ConnectorHttpAuthentication::None,
            })
            .expect("HTTP definition"),
        )
    }

    #[test]
    fn profile_advances_only_through_exact_non_noop_lineage() {
        let profile_id = ConnectorProfileId::new();
        let initial = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            profile_id,
            ConnectorRevisionId::new(),
            definition(1_000),
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("initial revision");
        let profile = ConnectorProfile::create(
            profile_id,
            ResourceName::parse("Incident Webhook").expect("name"),
            &initial,
        )
        .expect("profile");
        assert!(ConnectorRevision::successor(
            &initial,
            ConnectorRevisionId::new(),
            initial.definition.clone(),
            PrincipalId::new(),
            initial.created_at,
        )
        .is_err());
        let successor = ConnectorRevision::successor(
            &initial,
            ConnectorRevisionId::new(),
            definition(2_000),
            PrincipalId::new(),
            initial.created_at,
        )
        .expect("successor");
        let advanced = profile.advance(1, &successor).expect("advance");
        assert_eq!(advanced.aggregate_version, 2);
        assert_eq!(advanced.current_revision_id, successor.id);
        assert!(profile.advance(2, &successor).is_err());
    }
}
