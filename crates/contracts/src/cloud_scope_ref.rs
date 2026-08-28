use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A scope reference carried by an uncommitted Cloud fact.
///
/// Installation facts name their installation explicitly. Tenant facts name
/// their complete tenant lineage; the shared persistence boundary resolves and
/// verifies the owning installation from the canonical Organization row before
/// the fact can enter Audit or Outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CloudScopeRef {
    Installation {
        installation_id: Uuid,
    },
    Organization {
        organization_id: Uuid,
    },
    Project {
        organization_id: Uuid,
        project_id: Uuid,
    },
    Environment {
        organization_id: Uuid,
        project_id: Uuid,
        environment_id: Uuid,
    },
}

impl CloudScopeRef {
    pub fn installation(installation_id: Uuid) -> Result<Self, String> {
        Self::checked(Self::Installation { installation_id })
    }

    pub fn organization(organization_id: Uuid) -> Result<Self, String> {
        Self::checked(Self::Organization { organization_id })
    }

    pub fn project(organization_id: Uuid, project_id: Uuid) -> Result<Self, String> {
        Self::checked(Self::Project {
            organization_id,
            project_id,
        })
    }

    pub fn environment(
        organization_id: Uuid,
        project_id: Uuid,
        environment_id: Uuid,
    ) -> Result<Self, String> {
        Self::checked(Self::Environment {
            organization_id,
            project_id,
            environment_id,
        })
    }

    fn checked(value: Self) -> Result<Self, String> {
        value.validate()?;
        Ok(value)
    }

    pub fn validate(self) -> Result<(), String> {
        if self.installation_id().is_some_and(|value| value.is_nil())
            || self.organization_id().is_some_and(|value| value.is_nil())
            || self.project_id().is_some_and(|value| value.is_nil())
            || self.environment_id().is_some_and(|value| value.is_nil())
        {
            return Err("Cloud scope reference contains a nil identity".into());
        }
        Ok(())
    }

    pub const fn installation_id(self) -> Option<Uuid> {
        match self {
            Self::Installation { installation_id } => Some(installation_id),
            Self::Organization { .. } | Self::Project { .. } | Self::Environment { .. } => None,
        }
    }

    pub const fn organization_id(self) -> Option<Uuid> {
        match self {
            Self::Installation { .. } => None,
            Self::Organization { organization_id }
            | Self::Project {
                organization_id, ..
            }
            | Self::Environment {
                organization_id, ..
            } => Some(organization_id),
        }
    }

    pub const fn project_id(self) -> Option<Uuid> {
        match self {
            Self::Project { project_id, .. } | Self::Environment { project_id, .. } => {
                Some(project_id)
            }
            Self::Installation { .. } | Self::Organization { .. } => None,
        }
    }

    pub const fn environment_id(self) -> Option<Uuid> {
        match self {
            Self::Environment { environment_id, .. } => Some(environment_id),
            Self::Installation { .. } | Self::Organization { .. } | Self::Project { .. } => None,
        }
    }

    pub const fn kind(self) -> &'static str {
        match self {
            Self::Installation { .. } => "installation",
            Self::Organization { .. } => "organization",
            Self::Project { .. } => "project",
            Self::Environment { .. } => "environment",
        }
    }

    pub const fn is_tenant_scope(self) -> bool {
        !matches!(self, Self::Installation { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_scope_references_preserve_complete_tenant_lineage() {
        let organization_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let environment_id = Uuid::now_v7();
        let scope =
            CloudScopeRef::environment(organization_id, project_id, environment_id).expect("scope");
        assert_eq!(scope.organization_id(), Some(organization_id));
        assert_eq!(scope.project_id(), Some(project_id));
        assert_eq!(scope.environment_id(), Some(environment_id));
        assert_eq!(scope.installation_id(), None);
        assert!(scope.is_tenant_scope());
        assert_eq!(scope.kind(), "environment");
    }

    #[test]
    fn scope_reference_rejects_nil_ids_and_unknown_wire_shapes() {
        assert!(CloudScopeRef::installation(Uuid::nil()).is_err());
        assert!(CloudScopeRef::organization(Uuid::nil()).is_err());
        assert!(CloudScopeRef::project(Uuid::now_v7(), Uuid::nil()).is_err());
        assert!(CloudScopeRef::environment(Uuid::now_v7(), Uuid::now_v7(), Uuid::nil()).is_err());
        assert!(serde_json::from_str::<CloudScopeRef>(
            r#"{"kind":"workspace","organization_id":"00000000-0000-0000-0000-000000000001"}"#,
        )
        .is_err());
    }
}
