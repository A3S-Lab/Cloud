use super::*;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use chrono::Utc;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn github_repository_identity_is_canonical_and_url_confusion_fails_closed() {
    let first = GitRepository::parse(GitProvider::Github, "https://GITHUB.com/A3S-Lab/Cloud.GIT")
        .expect("GitHub repository");
    let second = GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud/")
        .expect("canonical GitHub repository");
    assert_eq!(first, second);
    assert_eq!(first.canonical_url(), "https://github.com/a3s-lab/cloud");
    assert_eq!(first.identity(), "github:github.com/a3s-lab/cloud");

    for confused in [
        "http://github.com/a3s-lab/cloud",
        "https://github.com@evil.example/a3s-lab/cloud",
        "https://github.com.evil.example/a3s-lab/cloud",
        "https://github.com/a3s-lab%2fother/cloud",
        "https://github.com/a3s-lab/cloud?token=secret",
        "https://github.com/a3s-lab/cloud/other",
    ] {
        assert!(
            GitRepository::parse(GitProvider::Github, confused).is_err(),
            "{confused}"
        );
    }
}

#[test]
fn commit_ids_are_full_and_canonical() {
    assert_eq!(
        GitCommitSha::parse(COMMIT.to_ascii_uppercase())
            .expect("commit")
            .as_str(),
        COMMIT
    );
    assert!(GitCommitSha::parse("0123456").is_err());
    assert!(GitCommitSha::parse(format!("{}z", "0".repeat(39))).is_err());
    assert!(GitCommitSha::parse("0".repeat(64)).is_ok());
}

#[test]
fn dockerfile_recipe_is_path_safe_ordered_and_digest_stable() {
    let first = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        "./services/api",
        "Dockerfile",
        Some("release"),
        vec!["linux/arm64".into(), "linux/amd64".into()],
    )
    .expect("build recipe");
    let second = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        "services/api",
        "Dockerfile",
        Some("release"),
        vec!["linux/amd64".into(), "linux/arm64".into()],
    )
    .expect("canonical build recipe");
    assert_eq!(first, second);
    assert_eq!(
        first
            .platforms()
            .iter()
            .map(BuildPlatform::as_str)
            .collect::<Vec<_>>(),
        vec!["linux/amd64", "linux/arm64"]
    );
    assert_eq!(
        first.digest().expect("digest"),
        second.digest().expect("digest")
    );
    assert_eq!(
        first.digest().expect("canonical digest"),
        "sha256:e2b4f203b431808a95e0ea8ae2e9728c37afec6b31e104c3f16e71b6d9baaac7"
    );

    for unsafe_path in ["../outside", "/absolute", "service\\Dockerfile", "a//b"] {
        assert!(BuildRecipe::dockerfile(
            BuildRecipe::SCHEMA,
            BuildRecipe::DOCKERFILE_KIND,
            unsafe_path,
            "Dockerfile",
            None,
            vec!["linux/amd64".into()],
        )
        .is_err());
    }
}

#[test]
fn source_revision_event_contains_immutable_metadata_only() {
    let repository = GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
        .expect("repository");
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        vec!["linux/amd64".into()],
    )
    .expect("recipe");
    let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        environment_id: EnvironmentId::new(),
        id: SourceRevisionId::new(),
        repository,
        commit_sha: GitCommitSha::parse(COMMIT).expect("commit"),
        recipe,
        accepted_at: Utc::now(),
    })
    .expect("source revision");
    assert_eq!(
        revision.source_identity_digest(),
        "sha256:638990d117ccb9a9cdd1072c508277dd432c37e6cc06615f3e288243e0301d68"
    );
    let event =
        SourceRevisionAccepted::envelope(&revision, uuid::Uuid::now_v7()).expect("source event");
    let payload = event.payload.to_string();
    assert!(payload.contains(COMMIT));
    assert!(payload.contains(&revision.recipe_digest));
    assert!(!payload.contains("credential"));
    assert!(!payload.contains("token"));
}
