use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, SourceRevisionId,
};
use crate::modules::sources::domain::{ExternalSourceRevision, ISourceRevisionRepository};
use crate::modules::sources::published::{
    SourceBuildInputSnapshot, ValidatedSourceBuildInputProjection,
};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceBuildInputQueryError {
    #[error("Source build input request is invalid: {0}")]
    Invalid(String),
    #[error("Source build input identity conflicts with durable state")]
    Conflict,
    #[error("Source build input failed integrity validation: {0}")]
    Integrity(String),
    #[error("Source build input storage failed: {0}")]
    Storage(String),
}

/// Sources-owned query boundary for one exact immutable build input.
#[async_trait]
pub trait ISourceBuildInputQueryPort: Send + Sync {
    async fn find_source_build_input(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<SourceBuildInputSnapshot>, SourceBuildInputQueryError>;
}

/// Owner-side adapter that keeps Source aggregate loading and validation
/// behind the published query boundary.
pub struct SourceBuildInputQueryService {
    revisions: Arc<dyn ISourceRevisionRepository>,
}

impl SourceBuildInputQueryService {
    pub fn new(revisions: Arc<dyn ISourceRevisionRepository>) -> Self {
        Self { revisions }
    }
}

#[async_trait]
impl ISourceBuildInputQueryPort for SourceBuildInputQueryService {
    async fn find_source_build_input(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<SourceBuildInputSnapshot>, SourceBuildInputQueryError> {
        if organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || environment_id.as_uuid().is_nil()
            || source_revision_id.as_uuid().is_nil()
        {
            return Err(SourceBuildInputQueryError::Invalid(
                "Source build input identity cannot contain nil IDs".into(),
            ));
        }
        let revision = match self
            .revisions
            .find(organization_id, source_revision_id)
            .await
        {
            Ok(revision) => revision,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(map_repository_error(error)),
        };
        let input =
            publish_source_build_input(&revision).map_err(SourceBuildInputQueryError::Integrity)?;
        if input.organization_id() != organization_id
            || input.project_id() != project_id
            || input.environment_id() != environment_id
            || input.source_revision_id() != source_revision_id
        {
            return Err(SourceBuildInputQueryError::Conflict);
        }
        Ok(Some(input))
    }
}

/// Projects one validated immutable Sources aggregate into its published build
/// language without exposing repository or aggregate semantics to consumers.
pub(crate) fn publish_source_build_input(
    revision: &ExternalSourceRevision,
) -> Result<SourceBuildInputSnapshot, String> {
    let revision = revision.clone().validate()?;
    SourceBuildInputSnapshot::from_validated_revision(ValidatedSourceBuildInputProjection {
        organization_id: revision.organization_id,
        project_id: revision.project_id,
        environment_id: revision.environment_id,
        source_revision_id: revision.id,
        repository: revision.repository,
        commit_sha: revision.commit_sha,
        recipe: revision.recipe,
        recipe_digest: revision.recipe_digest,
    })
}

fn map_repository_error(error: RepositoryError) -> SourceBuildInputQueryError {
    match error {
        RepositoryError::NotFound => SourceBuildInputQueryError::Storage(
            "Source repository returned an unexpected not-found error".into(),
        ),
        RepositoryError::Conflict(_)
        | RepositoryError::Forbidden(_)
        | RepositoryError::IdempotencyConflict => SourceBuildInputQueryError::Conflict,
        RepositoryError::Storage(message) => SourceBuildInputQueryError::Storage(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        GitCommitSha, IdempotencyRequest, IdempotentWrite,
    };
    use crate::modules::sources::domain::{AcceptSourceRevision, NewExternalSourceRevision};
    use crate::modules::sources::published::{BuildRecipe, GitProvider, GitRepository};
    use chrono::Utc;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn publishes_only_the_validated_exact_build_input() -> Result<(), String> {
        fn assert_send_sync<T: Send + Sync>() {}

        let revision = revision()?;
        let snapshot = publish_source_build_input(&revision)?;

        assert_send_sync::<SourceBuildInputSnapshot>();
        assert_eq!(snapshot.schema(), SourceBuildInputSnapshot::SCHEMA);
        assert_eq!(snapshot.organization_id(), revision.organization_id);
        assert_eq!(snapshot.project_id(), revision.project_id);
        assert_eq!(snapshot.environment_id(), revision.environment_id);
        assert_eq!(snapshot.source_revision_id(), revision.id);
        assert_eq!(snapshot.repository(), &revision.repository);
        assert_eq!(snapshot.commit_sha(), &revision.commit_sha);
        assert_eq!(snapshot.recipe(), &revision.recipe);
        assert_eq!(snapshot.recipe_digest().as_str(), revision.recipe_digest);
        Ok(())
    }

    #[test]
    fn refuses_to_publish_a_corrupted_source_aggregate() -> Result<(), String> {
        let mut revision = revision()?;
        revision.recipe_digest = format!("sha256:{}", "0".repeat(64));

        let error = publish_source_build_input(&revision)
            .expect_err("corrupted aggregate must remain inside Sources");
        assert_eq!(
            error,
            "source revision recipe digest does not match its recipe"
        );
        Ok(())
    }

    #[test]
    fn source_owner_rejects_nil_identity_before_publication() -> Result<(), String> {
        let error = ExternalSourceRevision::accept(NewExternalSourceRevision {
            id: SourceRevisionId::from_uuid(uuid::Uuid::nil()),
            ..new_revision()?
        })
        .expect_err("nil source identity must never form an aggregate");
        assert_eq!(error, "source revision identity cannot contain nil IDs");
        Ok(())
    }

    #[tokio::test]
    async fn owner_query_returns_only_the_exact_requested_scope() -> Result<(), String> {
        let revision = revision()?;
        let service = SourceBuildInputQueryService::new(Arc::new(StaticSourceRepository {
            revision: revision.clone(),
        }));

        let input = service
            .find_source_build_input(
                revision.organization_id,
                revision.project_id,
                revision.environment_id,
                revision.id,
            )
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "exact Source build input was absent".to_owned())?;
        assert_eq!(input.source_revision_id(), revision.id);

        assert_eq!(
            service
                .find_source_build_input(
                    revision.organization_id,
                    ProjectId::new(),
                    revision.environment_id,
                    revision.id,
                )
                .await,
            Err(SourceBuildInputQueryError::Conflict)
        );
        Ok(())
    }

    fn revision() -> Result<ExternalSourceRevision, String> {
        ExternalSourceRevision::accept(new_revision()?)
    }

    fn new_revision() -> Result<NewExternalSourceRevision, String> {
        Ok(NewExternalSourceRevision {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            id: SourceRevisionId::new(),
            repository: GitRepository::parse(
                GitProvider::Github,
                "https://github.com/A3S-Lab/Cloud.git",
            )?,
            commit_sha: GitCommitSha::parse(COMMIT)?,
            recipe: BuildRecipe::dockerfile(
                BuildRecipe::SCHEMA,
                BuildRecipe::DOCKERFILE_KIND,
                ".",
                "Dockerfile",
                None,
                vec!["linux/amd64".into()],
            )?,
            accepted_at: Utc::now(),
        })
    }

    struct StaticSourceRepository {
        revision: ExternalSourceRevision,
    }

    #[async_trait]
    impl ISourceRevisionRepository for StaticSourceRepository {
        async fn find(
            &self,
            _organization_id: OrganizationId,
            _source_revision_id: SourceRevisionId,
        ) -> Result<ExternalSourceRevision, RepositoryError> {
            Ok(self.revision.clone())
        }

        async fn replay_acceptance(
            &self,
            _idempotency: &IdempotencyRequest,
        ) -> Result<Option<ExternalSourceRevision>, RepositoryError> {
            Ok(None)
        }

        async fn accept(
            &self,
            _request: AcceptSourceRevision,
        ) -> Result<IdempotentWrite<ExternalSourceRevision>, RepositoryError> {
            Err(RepositoryError::Storage(
                "Source query fixture does not accept revisions".into(),
            ))
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Vec<ExternalSourceRevision>, RepositoryError> {
            Ok(Vec::new())
        }
    }
}
