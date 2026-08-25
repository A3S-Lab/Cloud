use super::{
    IPullRequestPreviewProjectionPort, ProjectCommittedPullRequestChange,
    PullRequestPreviewProjectionService,
};
use crate::modules::developer_workflows::domain::{
    AcceptPullRequestPreviewPolicyRevisionWrite, AcceptedPullRequestPreviewPolicyRevision,
    GitBranch, GithubInstallationRef, IPullRequestPreviewPolicyRepository,
    IPullRequestPreviewProjectionRepository, PreviewForkPolicy, PreviewQuota, PullRequestChange,
    PullRequestChangeKind, PullRequestPreviewPolicy, PullRequestPreviewPolicyContract,
    PullRequestPreviewPolicyRevisionAccepted, PullRequestPreviewProjectionOutcome,
};
use crate::modules::developer_workflows::infrastructure::{
    InMemoryPullRequestPreviewPolicyRepository, InMemoryPullRequestPreviewProjectionRepository,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GitCommitSha, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest, SourcePullRequestChangeId, SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn selects_policy_at_fact_time_and_terminally_replays_each_fact_id() {
    let fixture = Fixture::new();
    let policies = Arc::new(InMemoryPullRequestPreviewPolicyRepository::new());
    let previews = Arc::new(InMemoryPullRequestPreviewProjectionRepository::new());
    let service = service(policies.clone(), previews.clone());

    let before_policy = fixture.input('a', 5, 5, SourcePullRequestChangeId::new());
    let before_id = before_policy.source_pull_request_change_id;
    let before_digest = before_policy.fact_digest.clone();
    let ignored = service
        .project_committed_change(before_policy.clone())
        .await
        .expect("terminal no-policy projection");
    assert_eq!(
        ignored.value.outcome,
        PullRequestPreviewProjectionOutcome::NoApplicablePolicy
    );
    assert!(!ignored.replayed);

    let first = fixture.accept_policy(&policies, 1, None, 10, 86_400).await;
    let replayed_before = service
        .project_committed_change(before_policy)
        .await
        .expect("no-policy receipt replay");
    assert!(replayed_before.replayed);
    assert_eq!(replayed_before.value.policy_revision_id, None);

    let mut drifted = fixture.input('a', 5, 5, before_id);
    drifted.fact_digest = Sha256Digest::from_bytes(b"different owner fact");
    assert!(matches!(
        service.project_committed_change(drifted).await,
        Err(RepositoryError::Conflict(message))
            if message.contains("changed content or owner binding")
    ));
    assert_eq!(
        previews
            .find_receipt(fixture.organization_id, before_id)
            .await
            .expect("receipt query")
            .expect("receipt")
            .fact_digest,
        before_digest
    );

    let opened = fixture.input('b', 11, 11, SourcePullRequestChangeId::new());
    let created = service
        .project_committed_change(opened.clone())
        .await
        .expect("created Preview projection");
    assert_eq!(
        created.value.outcome,
        PullRequestPreviewProjectionOutcome::Created
    );
    assert_eq!(created.value.policy_revision_id, Some(first.id));
    assert!(!created.replayed);
    assert!(
        service
            .project_committed_change(opened)
            .await
            .expect("exact projection replay")
            .replayed
    );
}

#[tokio::test]
async fn source_facts_advance_lifecycle_without_rebinding_a_later_policy_revision() {
    let fixture = Fixture::new();
    let policies = Arc::new(InMemoryPullRequestPreviewPolicyRepository::new());
    let previews = Arc::new(InMemoryPullRequestPreviewProjectionRepository::new());
    let service = service(policies.clone(), previews.clone());

    let first = fixture.accept_policy(&policies, 1, None, 10, 86_400).await;
    service
        .project_committed_change(fixture.input('a', 20, 20, SourcePullRequestChangeId::new()))
        .await
        .expect("first Preview");
    let second = fixture
        .accept_policy(&policies, 2, Some(first.id), 25, 172_800)
        .await;
    assert_ne!(first.id, second.id);

    let updated = service
        .project_committed_change(fixture.input('b', 30, 30, SourcePullRequestChangeId::new()))
        .await
        .expect("updated Preview");
    assert_eq!(
        updated.value.outcome,
        PullRequestPreviewProjectionOutcome::Updated
    );
    assert_eq!(updated.value.policy_revision_id, Some(first.id));

    let preview = previews
        .find_preview(
            fixture.organization_id,
            fixture.project_id,
            fixture.source_environment_id,
            fixture.source_subscription_id,
            fixture.pull_request_id,
        )
        .await
        .expect("Preview query")
        .expect("Preview");
    assert_eq!(preview.policy_authority.revision_id, first.id);
    assert_eq!(
        preview.expires_at,
        timestamp(30) + TimeDelta::seconds(86_400)
    );
    assert_eq!(preview.aggregate_version, 2);

    let stale = service
        .project_committed_change(fixture.input('c', 31, 15, SourcePullRequestChangeId::new()))
        .await
        .expect("stale fact receipt");
    assert_eq!(
        stale.value.outcome,
        PullRequestPreviewProjectionOutcome::IgnoredStale
    );
    assert_eq!(stale.value.preview_aggregate_version, Some(2));
}

#[tokio::test]
async fn effective_policy_selection_orders_by_acceptance_time_then_revision_number() {
    let fixture = Fixture::new();
    let policies = Arc::new(InMemoryPullRequestPreviewPolicyRepository::new());
    let previews = Arc::new(InMemoryPullRequestPreviewProjectionRepository::new());
    let service = service(policies.clone(), previews);

    let first = fixture.accept_policy(&policies, 1, None, 10, 86_400).await;
    let second = fixture
        .accept_policy(&policies, 2, Some(first.id), 10, 172_800)
        .await;
    let third = fixture
        .accept_policy(&policies, 3, Some(second.id), 20, 259_200)
        .await;
    let regressed = fixture.policy_revision(4, 15, 345_600);
    assert!(matches!(
        policies
            .accept(fixture.policy_write(regressed, Some(third.id)))
            .await,
        Err(RepositoryError::Conflict(message))
            if message.contains("revision sequence is not monotonic")
    ));

    let projected = service
        .project_committed_change(fixture.input('d', 10, 10, SourcePullRequestChangeId::new()))
        .await
        .expect("event-time policy projection");
    assert_eq!(projected.value.policy_revision_id, Some(second.id));
    assert_ne!(projected.value.policy_revision_id, Some(third.id));
}

fn service(
    policies: Arc<InMemoryPullRequestPreviewPolicyRepository>,
    previews: Arc<InMemoryPullRequestPreviewProjectionRepository>,
) -> PullRequestPreviewProjectionService {
    let policies: Arc<dyn IPullRequestPreviewPolicyRepository> = policies;
    let previews: Arc<dyn IPullRequestPreviewProjectionRepository> = previews;
    PullRequestPreviewProjectionService::new(policies, previews)
}

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
    owner_principal_id: PrincipalId,
    pull_request_id: u64,
}

impl Fixture {
    fn new() -> Self {
        Self {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            source_environment_id: EnvironmentId::new(),
            source_subscription_id: SourceSubscriptionId::new(),
            owner_principal_id: PrincipalId::new(),
            pull_request_id: 1_000_042,
        }
    }

    async fn accept_policy(
        &self,
        repository: &InMemoryPullRequestPreviewPolicyRepository,
        revision_number: u64,
        expected_previous_revision_id: Option<
            crate::modules::shared_kernel::domain::PullRequestPreviewPolicyRevisionId,
        >,
        accepted_second: u32,
        lifetime_seconds: u32,
    ) -> AcceptedPullRequestPreviewPolicyRevision {
        let revision = self.policy_revision(revision_number, accepted_second, lifetime_seconds);
        repository
            .accept(self.policy_write(revision.clone(), expected_previous_revision_id))
            .await
            .expect("accept policy");
        revision
    }

    fn policy_revision(
        &self,
        revision_number: u64,
        accepted_second: u32,
        lifetime_seconds: u32,
    ) -> AcceptedPullRequestPreviewPolicyRevision {
        let contract = PullRequestPreviewPolicyContract::from_policy(PullRequestPreviewPolicy {
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_subscription_id: self.source_subscription_id,
            owner_principal_id: self.owner_principal_id,
            installation_id: GithubInstallationRef::parse(42).expect("installation"),
            base_repository: base_repository(),
            base_branch: GitBranch::parse("main").expect("branch"),
            lifetime_seconds,
            maximum_active_previews: 8,
            fork_policy: PreviewForkPolicy::Isolated,
            allow_protected_secrets_for_trusted_sources: true,
            quota: PreviewQuota {
                maximum_workloads: 4,
                cpu_millis: 2_000,
                memory_bytes: 1024 * 1024 * 1024,
                ephemeral_storage_bytes: 1024 * 1024 * 1024,
            },
        })
        .expect("policy contract");
        let revision = AcceptedPullRequestPreviewPolicyRevision::accept(
            self.source_environment_id,
            contract,
            revision_number,
            self.owner_principal_id,
            timestamp(accepted_second),
        )
        .expect("accepted policy");
        revision
    }

    fn policy_write(
        &self,
        revision: AcceptedPullRequestPreviewPolicyRevision,
        expected_previous_revision_id: Option<
            crate::modules::shared_kernel::domain::PullRequestPreviewPolicyRevisionId,
        >,
    ) -> AcceptPullRequestPreviewPolicyRevisionWrite {
        let request_id = Uuid::now_v7();
        AcceptPullRequestPreviewPolicyRevisionWrite {
            expected_previous_revision_id,
            event: PullRequestPreviewPolicyRevisionAccepted::envelope(&revision, request_id)
                .expect("policy event"),
            actor_principal_id: self.owner_principal_id,
            request_id,
            idempotency: IdempotencyRequest::new(
                "developer-preview-projection-tests",
                format!("policy-{}", revision.revision_number),
                revision.contract.digest().as_str().as_bytes(),
            )
            .expect("idempotency"),
            revision,
        }
    }

    fn input(
        &self,
        sha: char,
        fact_second: u32,
        provider_second: u32,
        source_pull_request_change_id: SourcePullRequestChangeId,
    ) -> ProjectCommittedPullRequestChange {
        ProjectCommittedPullRequestChange {
            source_pull_request_change_id,
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_environment_id: self.source_environment_id,
            source_subscription_id: self.source_subscription_id,
            change: PullRequestChange {
                installation_id: GithubInstallationRef::parse(42).expect("installation"),
                base_repository: base_repository(),
                base_branch: GitBranch::parse("main").expect("base branch"),
                head_repository: Some(base_repository()),
                head_branch: GitBranch::parse("feature/preview").expect("head branch"),
                head_commit_sha: GitCommitSha::parse(sha.to_string().repeat(40)).expect("commit"),
                pull_request_id: self.pull_request_id,
                pull_request_number: 42,
                kind: if sha == 'a' {
                    PullRequestChangeKind::Opened
                } else {
                    PullRequestChangeKind::Synchronized
                },
                merged: false,
                provider_created_at: timestamp(1),
                provider_updated_at: timestamp(provider_second),
            },
            fact_digest: Sha256Digest::from_bytes(
                format!("fact-{sha}-{fact_second}-{provider_second}").as_bytes(),
            ),
            fact_occurred_at: timestamp(fact_second),
        }
    }
}

fn base_repository() -> GitRepository {
    GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
        .expect("repository")
}

fn timestamp(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 26, 2, 0, second)
        .single()
        .expect("timestamp")
}
