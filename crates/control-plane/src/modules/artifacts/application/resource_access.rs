use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId, RepositoryError};
use std::sync::Arc;

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
        evaluator: &ResourceAccessEvaluator,
        not_found: &'static str,
    ) -> ApplicationResult<BuildRun> {
        let build_run = self
            .builds
            .find(organization_id, build_run_id)
            .await
            .map_err(|error| map_repository_error(error, not_found))?;
        if !build_run_is_visible(&build_run, evaluator) {
            return Err(ApplicationError::NotFound(not_found.into()));
        }
        Ok(build_run)
    }
}

fn build_run_is_visible(build_run: &BuildRun, evaluator: &ResourceAccessEvaluator) -> bool {
    if evaluator.is_organization_wide() {
        return true;
    }
    match (build_run.project_id(), build_run.environment_id()) {
        (Some(project_id), Some(environment_id)) => {
            evaluator.allows(ResourceGrantScope::Environment {
                project_id,
                environment_id,
            })
        }
        _ => false,
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
        let project =
            ResourceAccessEvaluator::restricted([ResourceGrantScope::Project { project_id }]);
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
            &ResourceAccessEvaluator::organization_wide()
        ));
    }
}
