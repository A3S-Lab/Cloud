use super::GithubSourceResolver;
use crate::modules::sources::domain::{
    GitProvider, GitReference, GitRepository, ISourceResolver, SourceResolutionError,
    SourceResolutionRequest,
};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::time::Duration;
use url::Url;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const TAG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[tokio::test]
async fn resolves_exact_branches_commits_and_annotated_tags() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("GitHub fixture listener");
    let address = listener.local_addr().expect("GitHub fixture address");
    let router = Router::new()
        .route("/repos/a3s-lab/cloud", get(repository))
        .route(
            "/repos/a3s-lab/cloud/git/ref/heads/feature/source",
            get(branch),
        )
        .route(
            "/repos/a3s-lab/cloud/git/commits/0123456789abcdef0123456789abcdef01234567",
            get(commit),
        )
        .route(
            "/repos/a3s-lab/cloud/git/ref/tags/v1.0.0",
            get(tag_reference),
        )
        .route(
            "/repos/a3s-lab/cloud/git/tags/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            get(annotated_tag),
        );
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("GitHub fixture server")
    });
    let resolver = GithubSourceResolver::for_test(
        Duration::from_secs(2),
        Url::parse(&format!("http://{address}/")).expect("fixture URL"),
    )
    .expect("GitHub resolver");
    let repository = github_repository();

    for reference in [
        GitReference::parse("branch", "feature/source").expect("branch"),
        GitReference::parse("commit", COMMIT).expect("commit"),
        GitReference::parse("tag", "v1.0.0").expect("tag"),
    ] {
        let resolved = resolver
            .resolve(&SourceResolutionRequest {
                repository: repository.clone(),
                reference,
            })
            .await
            .expect("resolved source");
        assert_eq!(resolved.repository, repository);
        assert_eq!(resolved.commit_sha.as_str(), COMMIT);
    }
    server.abort();
}

#[tokio::test]
async fn repository_identity_mismatch_fails_closed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("GitHub fixture listener");
    let address = listener.local_addr().expect("GitHub fixture address");
    let router = Router::new().route(
        "/repos/a3s-lab/cloud",
        get(|| async {
            Json(json!({
                "full_name": "attacker/cloud",
                "html_url": "https://github.com/attacker/cloud",
                "private": false
            }))
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("GitHub fixture server")
    });
    let resolver = GithubSourceResolver::for_test(
        Duration::from_secs(2),
        Url::parse(&format!("http://{address}/")).expect("fixture URL"),
    )
    .expect("GitHub resolver");
    let error = resolver
        .resolve(&SourceResolutionRequest {
            repository: github_repository(),
            reference: GitReference::parse("branch", "main").expect("branch"),
        })
        .await
        .expect_err("repository mismatch");
    assert!(matches!(error, SourceResolutionError::Unavailable));
    server.abort();
}

#[tokio::test]
async fn repository_redirects_are_not_followed() {
    use axum::extract::State;
    use axum::response::Redirect;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let followed = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("GitHub fixture listener");
    let address = listener.local_addr().expect("GitHub fixture address");
    let router = Router::new()
        .route(
            "/repos/a3s-lab/cloud",
            get(|| async { Redirect::permanent("/renamed") }),
        )
        .route(
            "/renamed",
            get(|State(followed): State<Arc<AtomicUsize>>| async move {
                followed.fetch_add(1, Ordering::SeqCst);
                repository().await
            }),
        )
        .with_state(Arc::clone(&followed));
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("GitHub fixture server")
    });
    let resolver = GithubSourceResolver::for_test(
        Duration::from_secs(2),
        Url::parse(&format!("http://{address}/")).expect("fixture URL"),
    )
    .expect("GitHub resolver");
    let error = resolver
        .resolve(&SourceResolutionRequest {
            repository: github_repository(),
            reference: GitReference::parse("branch", "main").expect("branch"),
        })
        .await
        .expect_err("repository redirect");
    assert!(matches!(error, SourceResolutionError::Unavailable));
    assert_eq!(followed.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
#[ignore = "requires public GitHub API access"]
async fn real_github_resolves_a_public_branch_then_confirms_the_pinned_commit() {
    let resolver = GithubSourceResolver::new(Duration::from_secs(15)).expect("GitHub resolver");
    let repository = github_repository();
    let branch = resolver
        .resolve(&SourceResolutionRequest {
            repository: repository.clone(),
            reference: GitReference::parse("branch", "main").expect("main branch"),
        })
        .await
        .expect("public GitHub branch");
    let pinned = resolver
        .resolve(&SourceResolutionRequest {
            repository,
            reference: GitReference::parse("commit", branch.commit_sha.as_str())
                .expect("pinned commit"),
        })
        .await
        .expect("public GitHub commit");
    assert_eq!(pinned.commit_sha, branch.commit_sha);
}

async fn repository() -> Json<Value> {
    Json(json!({
        "full_name": "A3S-Lab/Cloud",
        "html_url": "https://github.com/A3S-Lab/Cloud",
        "private": false
    }))
}

async fn branch() -> Json<Value> {
    Json(json!({
        "ref": "refs/heads/feature/source",
        "object": {"type": "commit", "sha": COMMIT}
    }))
}

async fn commit() -> Json<Value> {
    Json(json!({"sha": COMMIT}))
}

async fn tag_reference() -> Json<Value> {
    Json(json!({
        "ref": "refs/tags/v1.0.0",
        "object": {"type": "tag", "sha": TAG}
    }))
}

async fn annotated_tag() -> Json<Value> {
    Json(json!({
        "sha": TAG,
        "object": {"type": "commit", "sha": COMMIT}
    }))
}

fn github_repository() -> GitRepository {
    GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
        .expect("GitHub repository")
}
