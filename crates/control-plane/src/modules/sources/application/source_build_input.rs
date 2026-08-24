use crate::modules::sources::domain::ExternalSourceRevision;
use crate::modules::sources::published::{
    SourceBuildInputSnapshot, ValidatedSourceBuildInputProjection,
};

/// Projects one validated immutable Sources aggregate into its published build
/// language without exposing repository or aggregate semantics to consumers.
pub fn publish_source_build_input(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, GitCommitSha, OrganizationId, ProjectId, SourceRevisionId,
    };
    use crate::modules::sources::domain::NewExternalSourceRevision;
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
}
