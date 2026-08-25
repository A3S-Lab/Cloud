use super::ConnectorRevision;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId,
    PrincipalId, ProjectId, Sha256Digest,
};
use chrono::{DateTime, Utc};

pub const CONNECTOR_REVISION_REVOCATION_REASON_MAX_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRevisionRevocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub revision_number: u64,
    pub definition_digest: Sha256Digest,
    pub reason: String,
    pub revoked_by: PrincipalId,
    pub revoked_at: DateTime<Utc>,
}

impl ConnectorRevisionRevocation {
    pub fn new(
        revision: &ConnectorRevision,
        reason: impl Into<String>,
        revoked_by: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        revision.validate()?;
        let revocation = Self {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            profile_id: revision.profile_id,
            revision_id: revision.id,
            revision_number: revision.revision_number,
            definition_digest: revision.definition.digest().clone(),
            reason: normalize_connector_revision_revocation_reason(reason)?,
            revoked_by,
            revoked_at: canonical_timestamp(revoked_at),
        };
        revocation.validate_against(revision)?;
        Ok(revocation)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        revision_number: u64,
        definition_digest: Sha256Digest,
        reason: impl Into<String>,
        revoked_by: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let revocation = Self {
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision_id,
            revision_number,
            definition_digest,
            reason: reason.into(),
            revoked_by,
            revoked_at,
        };
        revocation.validate()?;
        Ok(revocation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.profile_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.revision_number == 0
            || self.revision_number > i64::MAX as u64
            || self.revoked_by.as_uuid().is_nil()
            || self.revoked_at != canonical_timestamp(self.revoked_at)
            || Sha256Digest::parse(self.definition_digest.as_str())? != self.definition_digest
            || normalize_connector_revision_revocation_reason(self.reason.as_str())? != self.reason
        {
            return Err("Connector revision revocation is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, revision: &ConnectorRevision) -> Result<(), String> {
        self.validate()?;
        revision.validate()?;
        if self.organization_id != revision.organization_id
            || self.project_id != revision.project_id
            || self.environment_id != revision.environment_id
            || self.profile_id != revision.profile_id
            || self.revision_id != revision.id
            || self.revision_number != revision.revision_number
            || self.definition_digest != *revision.definition.digest()
            || self.revoked_at < revision.created_at
        {
            return Err("Connector revision revocation does not match its exact revision".into());
        }
        Ok(())
    }
}

pub(crate) fn normalize_connector_revision_revocation_reason(
    reason: impl Into<String>,
) -> Result<String, String> {
    let reason = reason.into();
    let normalized = reason.trim().to_owned();
    if normalized.is_empty()
        || normalized.len() > CONNECTOR_REVISION_REVOCATION_REASON_MAX_BYTES
        || normalized.chars().any(char::is_control)
    {
        return Err("Connector revision revocation reason is invalid".into());
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDefinition,
        ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
        ConnectorHttpStatusPolicy,
    };
    use chrono::Duration;

    fn revision(created_at: DateTime<Utc>) -> ConnectorRevision {
        ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/revocation".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 1_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            created_at,
        )
        .expect("revision")
    }

    #[test]
    fn revocation_is_exact_canonical_and_monotonic() {
        let created_at = canonical_timestamp(Utc::now());
        let revision = revision(created_at);
        let revocation = ConnectorRevisionRevocation::new(
            &revision,
            "  compromised destination  ",
            PrincipalId::new(),
            created_at + Duration::seconds(1),
        )
        .expect("revocation");
        assert_eq!(revocation.reason, "compromised destination");
        revocation
            .validate_against(&revision)
            .expect("exact revision");

        assert!(ConnectorRevisionRevocation::new(
            &revision,
            "too early",
            PrincipalId::new(),
            created_at - Duration::seconds(1),
        )
        .is_err());
        assert!(ConnectorRevisionRevocation::new(
            &revision,
            "line\nbreak",
            PrincipalId::new(),
            created_at,
        )
        .is_err());
    }
}
