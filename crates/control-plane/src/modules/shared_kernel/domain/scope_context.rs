use super::{EnvironmentId, InstallationId, OrganizationId, ProjectId};
use a3s_cloud_contracts::CloudScopeRef;
use serde::{Deserialize, Serialize};

/// Exact Cloud authority scope carried across Application and owner-port
/// boundaries. It is identity, not ambient request state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeContext {
    Installation {
        installation_id: InstallationId,
    },
    Organization {
        installation_id: InstallationId,
        organization_id: OrganizationId,
    },
    Project {
        installation_id: InstallationId,
        organization_id: OrganizationId,
        project_id: ProjectId,
    },
    Environment {
        installation_id: InstallationId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl ScopeContext {
    /// Builds authority context only after persistence has resolved the
    /// reference's canonical owning Installation.
    pub(crate) fn from_resolved_reference(
        resolved_installation_id: InstallationId,
        reference: CloudScopeRef,
    ) -> Result<Self, String> {
        reference.validate()?;
        match reference {
            CloudScopeRef::Installation {
                installation_id: referenced,
            } => {
                if referenced != resolved_installation_id.as_uuid() {
                    return Err("Cloud scope reference belongs to another installation".into());
                }
                Self::installation(resolved_installation_id)
            }
            CloudScopeRef::Organization { organization_id } => Self::organization(
                resolved_installation_id,
                OrganizationId::from_uuid(organization_id),
            ),
            CloudScopeRef::Project {
                organization_id,
                project_id,
            } => Self::project(
                resolved_installation_id,
                OrganizationId::from_uuid(organization_id),
                ProjectId::from_uuid(project_id),
            ),
            CloudScopeRef::Environment {
                organization_id,
                project_id,
                environment_id,
            } => Self::environment(
                resolved_installation_id,
                OrganizationId::from_uuid(organization_id),
                ProjectId::from_uuid(project_id),
                EnvironmentId::from_uuid(environment_id),
            ),
        }
    }

    pub fn installation(installation_id: InstallationId) -> Result<Self, String> {
        Self::checked(Self::Installation { installation_id })
    }

    pub fn organization(
        installation_id: InstallationId,
        organization_id: OrganizationId,
    ) -> Result<Self, String> {
        Self::checked(Self::Organization {
            installation_id,
            organization_id,
        })
    }

    pub fn project(
        installation_id: InstallationId,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Self, String> {
        Self::checked(Self::Project {
            installation_id,
            organization_id,
            project_id,
        })
    }

    pub fn environment(
        installation_id: InstallationId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Self, String> {
        Self::checked(Self::Environment {
            installation_id,
            organization_id,
            project_id,
            environment_id,
        })
    }

    fn checked(value: Self) -> Result<Self, String> {
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.installation_id().as_uuid().is_nil()
            || self
                .organization_id()
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .project_id()
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .environment_id()
                .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("Cloud scope context contains a nil authority identity".into());
        }
        Ok(())
    }

    pub const fn installation_id(self) -> InstallationId {
        match self {
            Self::Installation { installation_id }
            | Self::Organization {
                installation_id, ..
            }
            | Self::Project {
                installation_id, ..
            }
            | Self::Environment {
                installation_id, ..
            } => installation_id,
        }
    }

    pub const fn organization_id(self) -> Option<OrganizationId> {
        match self {
            Self::Installation { .. } => None,
            Self::Organization {
                organization_id, ..
            }
            | Self::Project {
                organization_id, ..
            }
            | Self::Environment {
                organization_id, ..
            } => Some(organization_id),
        }
    }

    pub const fn project_id(self) -> Option<ProjectId> {
        match self {
            Self::Project { project_id, .. } | Self::Environment { project_id, .. } => {
                Some(project_id)
            }
            Self::Installation { .. } | Self::Organization { .. } => None,
        }
    }

    pub const fn environment_id(self) -> Option<EnvironmentId> {
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

    pub const fn reference(self) -> CloudScopeRef {
        match self {
            Self::Installation { installation_id } => CloudScopeRef::Installation {
                installation_id: installation_id.as_uuid(),
            },
            Self::Organization {
                organization_id, ..
            } => CloudScopeRef::Organization {
                organization_id: organization_id.as_uuid(),
            },
            Self::Project {
                organization_id,
                project_id,
                ..
            } => CloudScopeRef::Project {
                organization_id: organization_id.as_uuid(),
                project_id: project_id.as_uuid(),
            },
            Self::Environment {
                organization_id,
                project_id,
                environment_id,
                ..
            } => CloudScopeRef::Environment {
                organization_id: organization_id.as_uuid(),
                project_id: project_id.as_uuid(),
                environment_id: environment_id.as_uuid(),
            },
        }
    }

    /// Returns whether `self` is the same scope as, or an ancestor of,
    /// `candidate`. UUID equality at a lower level never bypasses its parents.
    pub fn contains(self, candidate: Self) -> Result<bool, String> {
        self.validate()?;
        candidate.validate()?;
        if self.installation_id() != candidate.installation_id() {
            return Ok(false);
        }
        Ok(match self {
            Self::Installation { .. } => true,
            Self::Organization {
                organization_id, ..
            } => candidate.organization_id() == Some(organization_id),
            Self::Project {
                organization_id,
                project_id,
                ..
            } => {
                candidate.organization_id() == Some(organization_id)
                    && candidate.project_id() == Some(project_id)
            }
            Self::Environment {
                organization_id,
                project_id,
                environment_id,
                ..
            } => {
                candidate.organization_id() == Some(organization_id)
                    && candidate.project_id() == Some(project_id)
                    && candidate.environment_id() == Some(environment_id)
            }
        })
    }

    /// Intersects two independently admitted scopes. A result exists only
    /// when one scope is an ancestor of the other, and it is always the
    /// narrower operand.
    pub fn intersection(self, other: Self) -> Result<Option<Self>, String> {
        if self.contains(other)? {
            Ok(Some(other))
        } else if other.contains(self)? {
            Ok(Some(self))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn hierarchy_intersection_only_narrows() {
        let installation_id = InstallationId::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let installation = ScopeContext::installation(installation_id).expect("installation");
        let organization =
            ScopeContext::organization(installation_id, organization_id).expect("organization");
        let project =
            ScopeContext::project(installation_id, organization_id, project_id).expect("project");
        let environment =
            ScopeContext::environment(installation_id, organization_id, project_id, environment_id)
                .expect("environment");

        assert!(installation.contains(environment).expect("contains"));
        assert!(organization.contains(environment).expect("contains"));
        assert!(project.contains(environment).expect("contains"));
        assert_eq!(
            organization
                .intersection(environment)
                .expect("intersection"),
            Some(environment)
        );
    }

    #[test]
    fn identical_child_ids_cannot_cross_parent_scope() {
        let installation_id = InstallationId::new();
        let project_id = ProjectId::new();
        let first = ScopeContext::project(installation_id, OrganizationId::new(), project_id)
            .expect("first");
        let second = ScopeContext::project(installation_id, OrganizationId::new(), project_id)
            .expect("second");
        assert!(!first.contains(second).expect("contains"));
        assert_eq!(first.intersection(second).expect("intersection"), None);
    }

    #[test]
    fn rejects_nil_identity_at_every_level() {
        assert!(ScopeContext::installation(InstallationId::from_uuid(Uuid::nil())).is_err());
        assert!(ScopeContext::organization(
            InstallationId::new(),
            OrganizationId::from_uuid(Uuid::nil())
        )
        .is_err());
        assert!(ScopeContext::project(
            InstallationId::new(),
            OrganizationId::new(),
            ProjectId::from_uuid(Uuid::nil())
        )
        .is_err());
        assert!(ScopeContext::environment(
            InstallationId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::from_uuid(Uuid::nil())
        )
        .is_err());
    }

    #[test]
    fn admitted_scope_round_trips_through_one_public_reference_shape() {
        let installation_id = InstallationId::new();
        let value = ScopeContext::environment(
            installation_id,
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
        )
        .expect("scope");
        assert_eq!(
            ScopeContext::from_resolved_reference(installation_id, value.reference()),
            Ok(value)
        );
        assert!(
            ScopeContext::from_resolved_reference(InstallationId::new(), value.reference()).is_ok()
        );

        let installation = ScopeContext::installation(installation_id).expect("installation");
        assert!(ScopeContext::from_resolved_reference(
            InstallationId::new(),
            installation.reference()
        )
        .is_err());
    }
}
