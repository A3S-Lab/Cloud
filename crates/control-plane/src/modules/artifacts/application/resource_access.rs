use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One Artifacts visibility selector projected from an Identity decision.
///
/// This is not an authorization grant. It deliberately models only the two
/// ownership shapes understood by BuildRun: a whole project or one exact
/// environment. Node grants never enter the Artifacts bounded context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ArtifactAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl ArtifactAccessScope {
    fn allows(self, project_id: ProjectId, environment_id: EnvironmentId) -> bool {
        match self {
            Self::Project {
                project_id: granted,
            } => granted == project_id,
            Self::Environment {
                project_id: granted_project,
                environment_id: granted_environment,
            } => granted_project == project_id && granted_environment == environment_id,
        }
    }
}

/// Artifacts-owned projection of an already-authorized request.
///
/// Identity remains the sole authorization authority. The root anti-corruption
/// layer narrows Identity grants into this immutable value;
/// Artifacts then applies its own BuildRun ownership semantics without
/// importing Identity roles, grant types, repositories, or evaluators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<ArtifactAccessScope>,
}

impl ArtifactAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(
        granted_scopes: impl IntoIterator<Item = ArtifactAccessScope>,
    ) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn environment_is_visible(
        &self,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|granted| granted.allows(project_id, environment_id))
    }

    pub(crate) const fn organization_build_is_visible(&self) -> bool {
        self.is_organization_wide()
    }

    #[cfg(test)]
    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = ArtifactAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }
}

/// Resolves an indirect BuildRun through the Artifacts authority before grant evaluation.
///
/// External-source builds carry a canonical project/environment scope. Hosted Asset release
/// builds are organization-scoped today, so restricted memberships fail closed instead of being
/// assigned a synthetic project or copied into an Identity ownership index.
#[derive(Clone)]
pub(crate) struct BuildRunResourceAccess {
    builds: Arc<dyn IBuildRunRepository>,
}

impl BuildRunResourceAccess {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }

    pub async fn build_run(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
        access: &ArtifactAccess,
        not_found: &'static str,
    ) -> ApplicationResult<BuildRun> {
        let build_run = self
            .builds
            .find(organization_id, build_run_id)
            .await
            .map_err(|error| map_repository_error(error, not_found))?;
        if !build_run_is_visible(&build_run, access) {
            return Err(ApplicationError::NotFound(not_found.into()));
        }
        Ok(build_run)
    }
}

fn build_run_is_visible(build_run: &BuildRun, access: &ArtifactAccess) -> bool {
    match (build_run.project_id(), build_run.environment_id()) {
        (Some(project_id), Some(environment_id)) => {
            access.environment_is_visible(project_id, environment_id)
        }
        _ => access.organization_build_is_visible(),
    }
}

fn map_repository_error(error: RepositoryError, not_found: &'static str) -> ApplicationError {
    match error {
        RepositoryError::NotFound => ApplicationError::NotFound(not_found.into()),
        error => error.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, EnvironmentId, ProjectId, SourceRevisionId,
    };
    use chrono::Utc;

    #[test]
    fn external_builds_use_environment_scope_and_asset_builds_fail_closed() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let project = ArtifactAccess::restricted([ArtifactAccessScope::Project { project_id }]);
        let external = BuildRun::reserve(
            organization_id,
            project_id,
            environment_id,
            SourceRevisionId::new(),
            Utc::now(),
        );
        assert!(build_run_is_visible(&external, &project));

        let asset = BuildRun::reserve_asset_release(
            organization_id,
            AssetId::new(),
            AssetReleaseId::new(),
            Utc::now(),
        );
        assert!(!build_run_is_visible(&asset, &project));
        assert!(build_run_is_visible(
            &asset,
            &ArtifactAccess::organization_wide()
        ));
    }

    #[test]
    fn environment_access_is_exact_project_scoped_and_canonicalized() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let exact = ArtifactAccessScope::Environment {
            project_id,
            environment_id,
        };
        let access = ArtifactAccess::restricted([exact, exact]);

        assert_eq!(access.granted_scopes().collect::<Vec<_>>(), [exact]);
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(!access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), environment_id));
        assert!(!access.organization_build_is_visible());
    }
}
