use super::GitSourceCheckout;
use crate::modules::sources::domain::{
    GitCommitSha, GitProvider, GitReference, GitRepository, ISourceCheckout, ISourceResolver,
    SourceCheckoutError, SourceCheckoutRequest, SourceResolutionRequest,
};
use crate::modules::sources::GithubSourceResolver;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test]
async fn checkout_pins_the_commit_and_replays_immutable_content() {
    let fixture = GitFixture::new();
    let first_commit = fixture.commit("message.txt", "first\n", "first");
    fixture.push_main();
    let second_commit = fixture.commit("message.txt", "second\n", "second");
    fixture.push_main();

    let checkout_root = fixture.root.path().join("checkouts");
    let checkout = fixture.checkout(&checkout_root, 1_000);
    let checkout_id = Uuid::now_v7();
    let request = source_request(checkout_id, &first_commit);
    let accepted = checkout.checkout(&request).await.expect("pinned checkout");

    assert_eq!(accepted.checkout_id, checkout_id);
    assert_eq!(accepted.commit_sha.as_str(), first_commit);
    assert_eq!(
        tokio::fs::read_to_string(accepted.directory.join("message.txt"))
            .await
            .expect("checked-out file"),
        "first\n"
    );
    assert!(!accepted.directory.join(".git").exists());
    assert!(accepted.content_digest.starts_with("sha256:"));

    let replay = checkout.checkout(&request).await.expect("checkout replay");
    assert_eq!(replay, accepted);

    let conflict = checkout
        .checkout(&source_request(checkout_id, &second_commit))
        .await
        .expect_err("conflicting checkout");
    assert!(matches!(conflict, SourceCheckoutError::Conflict));

    tokio::fs::write(accepted.directory.join("message.txt"), "tampered\n")
        .await
        .expect("tamper with checkout");
    let tampered = checkout
        .checkout(&request)
        .await
        .expect_err("tampered checkout");
    assert!(matches!(tampered, SourceCheckoutError::Integrity(_)));

    checkout.remove(checkout_id).await.expect("remove checkout");
    checkout
        .remove(checkout_id)
        .await
        .expect("idempotent remove");
    assert!(!checkout_root.join(checkout_id.to_string()).exists());
}

#[tokio::test]
async fn checkout_rejects_source_trees_that_exceed_the_file_limit() {
    let fixture = GitFixture::new();
    std::fs::write(fixture.work.join("first.txt"), "first\n").expect("first file");
    std::fs::write(fixture.work.join("second.txt"), "second\n").expect("second file");
    let commit = fixture.commit_all("two files");
    fixture.push_main();
    let checkout_root = fixture.root.path().join("checkouts");
    let checkout = fixture.checkout(&checkout_root, 1);

    let error = checkout
        .checkout(&source_request(Uuid::now_v7(), &commit))
        .await
        .expect_err("file limit");
    assert!(matches!(error, SourceCheckoutError::Integrity(_)));
    assert_staging_is_empty(&checkout_root);
}

#[tokio::test]
async fn checkout_rejects_source_trees_that_exceed_the_content_limit() {
    let fixture = GitFixture::new();
    let commit = fixture.commit("large.txt", "bounded content\n", "large file");
    fixture.push_main();
    let checkout_root = fixture.root.path().join("checkouts");
    let checkout = GitSourceCheckout::for_test(
        &checkout_root,
        Duration::from_secs(10),
        1_000,
        4,
        &fixture.remote,
    )
    .expect("checkout adapter");

    let error = checkout
        .checkout(&source_request(Uuid::now_v7(), &commit))
        .await
        .expect_err("content limit");
    assert!(matches!(error, SourceCheckoutError::Integrity(_)));
    assert_staging_is_empty(&checkout_root);
}

#[cfg(unix)]
#[tokio::test]
async fn checkout_rejects_symlinks_that_escape_the_source_root() {
    use std::os::unix::fs::symlink;

    let fixture = GitFixture::new();
    symlink("../outside", fixture.work.join("escape")).expect("escaping symlink");
    let commit = fixture.commit_all("escaping symlink");
    fixture.push_main();
    let checkout_root = fixture.root.path().join("checkouts");
    let checkout = fixture.checkout(&checkout_root, 1_000);

    let error = checkout
        .checkout(&source_request(Uuid::now_v7(), &commit))
        .await
        .expect_err("escaping symlink");
    assert!(matches!(error, SourceCheckoutError::Integrity(_)));
    assert_staging_is_empty(&checkout_root);
}

#[cfg(unix)]
#[tokio::test]
async fn checkout_preserves_symlinks_that_remain_inside_the_source_root() {
    use std::os::unix::fs::symlink;

    let fixture = GitFixture::new();
    std::fs::write(fixture.work.join("target.txt"), "target\n").expect("symlink target");
    symlink("target.txt", fixture.work.join("link.txt")).expect("internal symlink");
    let commit = fixture.commit_all("internal symlink");
    fixture.push_main();
    let checkout_root = fixture.root.path().join("checkouts");
    let checkout = fixture.checkout(&checkout_root, 1_000);
    let request = source_request(Uuid::now_v7(), &commit);

    let accepted = checkout.checkout(&request).await.expect("internal symlink");
    assert_eq!(
        tokio::fs::read_link(accepted.directory.join("link.txt"))
            .await
            .expect("checked-out symlink"),
        std::path::PathBuf::from("target.txt")
    );
    assert_eq!(
        checkout.checkout(&request).await.expect("symlink replay"),
        accepted
    );
}

#[tokio::test]
async fn checkout_rejects_gitlinks_without_initializing_submodules() {
    let fixture = GitFixture::new();
    let dependency = fixture.root.path().join("dependency");
    git(
        fixture.root.path(),
        &["init", "--initial-branch=main", path(dependency.as_path())],
    );
    git(&dependency, &["config", "user.name", "A3S Cloud Test"]);
    git(
        &dependency,
        &["config", "user.email", "cloud-test@example.invalid"],
    );
    std::fs::write(dependency.join("dependency.txt"), "dependency\n").expect("dependency file");
    git(&dependency, &["add", "--", "dependency.txt"]);
    git(&dependency, &["commit", "--quiet", "-m", "dependency"]);
    git(
        &fixture.work,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            path(&dependency),
            "vendor/dependency",
        ],
    );
    let commit = fixture.commit_all("submodule");
    fixture.push_main();
    let checkout_root = fixture.root.path().join("checkouts");
    let checkout = fixture.checkout(&checkout_root, 1_000);

    let error = checkout
        .checkout(&source_request(Uuid::now_v7(), &commit))
        .await
        .expect_err("submodule");
    assert!(matches!(error, SourceCheckoutError::Integrity(_)));
    assert_staging_is_empty(&checkout_root);
}

#[tokio::test]
#[ignore = "requires public GitHub and Git protocol access"]
async fn real_github_checkout_materializes_the_resolved_commit_without_git_metadata() {
    let resolver = GithubSourceResolver::new(Duration::from_secs(15)).expect("GitHub resolver");
    let repository = github_repository();
    let resolved = resolver
        .resolve(&SourceResolutionRequest {
            repository: repository.clone(),
            reference: GitReference::parse("branch", "main").expect("main branch"),
        })
        .await
        .expect("public GitHub branch");
    let directory = tempfile::tempdir().expect("public checkout directory");
    let checkout = GitSourceCheckout::new(
        directory.path(),
        Duration::from_secs(60),
        100_000,
        512 * 1024 * 1024,
    )
    .expect("Git checkout");
    let request =
        SourceCheckoutRequest::new(Uuid::now_v7(), repository, resolved.commit_sha.clone())
            .expect("checkout request");

    let accepted = checkout
        .checkout(&request)
        .await
        .expect("public Git checkout");
    assert_eq!(accepted.commit_sha, resolved.commit_sha);
    assert!(accepted.directory.join("README.md").is_file());
    assert!(!accepted.directory.join(".git").exists());
    assert_eq!(
        checkout.checkout(&request).await.expect("public replay"),
        accepted
    );
    checkout
        .remove(request.checkout_id)
        .await
        .expect("public checkout cleanup");
}

fn source_request(checkout_id: Uuid, commit: &str) -> SourceCheckoutRequest {
    SourceCheckoutRequest::new(
        checkout_id,
        github_repository(),
        GitCommitSha::parse(commit).expect("commit"),
    )
    .expect("checkout request")
}

struct GitFixture {
    root: TempDir,
    remote: std::path::PathBuf,
    work: std::path::PathBuf,
}

impl GitFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("Git fixture");
        let remote = root.path().join("remote.git");
        let work = root.path().join("work");
        git(root.path(), &["init", "--bare", path(&remote)]);
        git(root.path(), &["init", "--initial-branch=main", path(&work)]);
        git(&work, &["config", "user.name", "A3S Cloud Test"]);
        git(
            &work,
            &["config", "user.email", "cloud-test@example.invalid"],
        );
        git(&work, &["remote", "add", "origin", path(&remote)]);
        Self { root, remote, work }
    }

    fn checkout(&self, root: &Path, max_files: usize) -> GitSourceCheckout {
        GitSourceCheckout::for_test(
            root,
            Duration::from_secs(10),
            max_files,
            16 * 1024 * 1024,
            &self.remote,
        )
        .expect("checkout adapter")
    }

    fn commit(&self, name: &str, content: &str, message: &str) -> String {
        std::fs::write(self.work.join(name), content).expect("write Git fixture");
        git(&self.work, &["add", "--", name]);
        git(&self.work, &["commit", "--quiet", "-m", message]);
        git_output(&self.work, &["rev-parse", "HEAD"])
    }

    fn commit_all(&self, message: &str) -> String {
        git(&self.work, &["add", "--all"]);
        git(&self.work, &["commit", "--quiet", "-m", message]);
        git_output(&self.work, &["rev-parse", "HEAD"])
    }

    fn push_main(&self) {
        git(
            &self.work,
            &["push", "--quiet", "--force", "origin", "main"],
        );
    }
}

fn git(directory: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(directory)
        .args(args)
        .status()
        .expect("run Git fixture command");
    assert!(status.success(), "Git fixture command failed: {args:?}");
}

fn git_output(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run Git fixture query");
    assert!(
        output.status.success(),
        "Git fixture query failed: {args:?}"
    );
    String::from_utf8(output.stdout)
        .expect("Git fixture UTF-8")
        .trim()
        .to_owned()
}

fn path(value: &Path) -> &str {
    value.to_str().expect("fixture path")
}

fn github_repository() -> GitRepository {
    GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
        .expect("GitHub repository")
}

fn assert_staging_is_empty(root: &Path) {
    let entries = std::fs::read_dir(root)
        .expect("checkout root")
        .collect::<Result<Vec<_>, _>>()
        .expect("checkout root entries");
    assert!(entries.is_empty(), "failed checkout left staging content");
}
