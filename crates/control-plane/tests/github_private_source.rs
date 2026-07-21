use a3s_cloud_control_plane::modules::sources::domain::{
    GitProvider, GitReference, GitRepository, GithubInstallationId, GithubInstallationTokenRequest,
    IGithubInstallationTokenService, ISourceCheckout, ISourceResolver, SourceCheckoutRequest,
    SourceResolutionRequest,
};
use a3s_cloud_control_plane::modules::sources::{
    GitSourceCheckout, GithubInstallationTokenIssuer, GithubSourceResolver,
};
use chrono::Utc;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires a real GitHub App installation and private repository"]
async fn real_github_installation_token_resolves_and_checks_out_a_private_repository(
) -> Result<(), Box<dyn std::error::Error>> {
    const PRIVATE_KEY_ENV: &str = "A3S_CLOUD_TEST_GITHUB_APP_PRIVATE_KEY";

    let client_id = required("A3S_CLOUD_TEST_GITHUB_APP_CLIENT_ID")?;
    let installation_id = required("A3S_CLOUD_TEST_GITHUB_INSTALLATION_ID")?.parse::<u64>()?;
    let repository_url = required("A3S_CLOUD_TEST_GITHUB_PRIVATE_REPOSITORY")?;
    let branch = required("A3S_CLOUD_TEST_GITHUB_PRIVATE_BRANCH")?;
    if std::env::var_os(PRIVATE_KEY_ENV).is_none() {
        return Err(test_error(format!("{PRIVATE_KEY_ENV} is required")));
    }
    let repository =
        GitRepository::parse(GitProvider::Github, &repository_url).map_err(test_error)?;
    let issuer =
        GithubInstallationTokenIssuer::new(Duration::from_secs(30), client_id, PRIVATE_KEY_ENV)
            .map_err(test_error)?;
    let credential = issuer
        .issue(GithubInstallationTokenRequest {
            installation_id: GithubInstallationId::parse(installation_id).map_err(test_error)?,
            repository: repository.clone(),
            requested_at: Utc::now(),
        })
        .await?;
    let resolver = GithubSourceResolver::new(Duration::from_secs(30)).map_err(test_error)?;
    let resolved = resolver
        .resolve(
            &SourceResolutionRequest {
                repository: repository.clone(),
                reference: GitReference::parse("branch", branch).map_err(test_error)?,
            },
            Some(&credential),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let checkout = GitSourceCheckout::new(
        directory.path(),
        Duration::from_secs(120),
        100_000,
        512 * 1024 * 1024,
    )
    .map_err(test_error)?;
    let request =
        SourceCheckoutRequest::new(Uuid::now_v7(), repository, resolved.commit_sha.clone())
            .map_err(test_error)?;

    let accepted = checkout.checkout(&request, Some(&credential)).await?;
    assert_eq!(accepted.commit_sha, resolved.commit_sha);
    assert!(!accepted.directory.join(".git").exists());
    drop(credential);
    assert_eq!(checkout.checkout(&request, None).await?, accepted);
    checkout.remove(request.checkout_id).await?;
    Ok(())
}

fn required(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    std::env::var(name).map_err(|_| test_error(format!("{name} is required")))
}

fn test_error(message: String) -> Box<dyn std::error::Error> {
    std::io::Error::other(message).into()
}
