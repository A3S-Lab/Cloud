use super::{
    reconcile_pull_request_preview, PreviewCleanupReason, PreviewForkPolicy, PreviewQuota,
    PreviewReconcileOutcome, PullRequestPreview, PullRequestPreviewPolicy,
    PullRequestPreviewStatus, MAX_PREVIEW_LIFETIME_SECONDS, MIN_PREVIEW_LIFETIME_SECONDS,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, SourceSubscriptionId,
};
use crate::modules::sources::domain::{
    GitCommitSha, GitProvider, GitReference, GitRepository, GithubInstallationId,
    PullRequestChangeKind, VerifiedPullRequestChange, WebhookDeliveryId,
};
use chrono::{DateTime, TimeDelta, TimeZone, Timelike, Utc};

#[test]
fn derives_stable_preview_and_ordinary_environment_identities_with_closed_bounds() {
    let policy = policy(PreviewForkPolicy::Isolated);
    policy.validate().expect("preview policy");
    let first = PullRequestPreview::preview_id_for(&policy, 1_000_042, 42).expect("preview ID");
    let second = PullRequestPreview::preview_id_for(&policy, 1_000_042, 42).expect("preview ID");
    assert_eq!(first, second);
    assert_eq!(
        PullRequestPreview::environment_id_for(first),
        PullRequestPreview::environment_id_for(second)
    );

    let created = reconcile_pull_request_preview(
        &policy,
        None,
        &change(
            PullRequestChangeKind::Opened,
            timestamp(10),
            'a',
            Some(base_repository()),
            false,
        ),
    )
    .expect("preview reconciliation")
    .preview
    .expect("preview");
    assert_eq!(
        created.environment_name(),
        format!("pr-42-{}", &first.as_uuid().simple().to_string()[..8])
    );

    let mut invalid = policy.clone();
    invalid.lifetime_seconds = MIN_PREVIEW_LIFETIME_SECONDS - 1;
    assert!(invalid.validate().is_err());
    invalid.lifetime_seconds = MAX_PREVIEW_LIFETIME_SECONDS + 1;
    assert!(invalid.validate().is_err());
    invalid = policy;
    invalid.quota.memory_bytes += 1;
    assert!(invalid.validate().is_err());
}

#[test]
fn creates_one_trusted_preview_and_converges_duplicate_deliveries() {
    let policy = policy(PreviewForkPolicy::Isolated);
    let opened = change(
        PullRequestChangeKind::Opened,
        timestamp(10),
        'a',
        Some(base_repository()),
        false,
    );
    let created = reconcile_pull_request_preview(&policy, None, &opened).expect("created preview");
    assert_eq!(created.outcome, PreviewReconcileOutcome::Created);
    let preview = created.preview.expect("preview");
    preview.validate().expect("valid preview");
    assert!(preview.status.is_active());
    assert!(!preview.is_fork());
    assert!(preview.protected_secrets_eligible());
    assert_eq!(
        preview.expires_at,
        timestamp(10) + TimeDelta::seconds(i64::from(policy.lifetime_seconds))
    );

    let replay = reconcile_pull_request_preview(&policy, Some(&preview), &opened)
        .expect("duplicate preview event");
    assert_eq!(replay.outcome, PreviewReconcileOutcome::IgnoredDuplicate);
    assert_eq!(replay.preview, Some(preview));
}

#[test]
fn denies_known_forks_or_admits_them_without_protected_secrets() {
    let fork = change(
        PullRequestChangeKind::Opened,
        timestamp(10),
        'a',
        Some(fork_repository()),
        false,
    );
    let denied = reconcile_pull_request_preview(&policy(PreviewForkPolicy::Deny), None, &fork)
        .expect("denied fork");
    assert_eq!(denied.outcome, PreviewReconcileOutcome::ForkDenied);
    assert!(denied.preview.is_none());

    let isolated =
        reconcile_pull_request_preview(&policy(PreviewForkPolicy::Isolated), None, &fork)
            .expect("isolated fork")
            .preview
            .expect("fork preview");
    assert!(isolated.is_fork());
    assert!(!isolated.protected_secrets_eligible());

    let deny_policy = policy(PreviewForkPolicy::Deny);
    let trusted = apply(
        &deny_policy,
        [&change(
            PullRequestChangeKind::Opened,
            timestamp(10),
            'a',
            Some(base_repository()),
            false,
        )],
    );
    let newer_fork = change(
        PullRequestChangeKind::Synchronized,
        timestamp(20),
        'b',
        Some(fork_repository()),
        false,
    );
    let cleanup = reconcile_pull_request_preview(&deny_policy, Some(&trusted), &newer_fork)
        .expect("existing trusted preview rejects a later fork");
    assert_eq!(cleanup.outcome, PreviewReconcileOutcome::ForkDenied);
    let cleanup = cleanup.preview.expect("fork cleanup decision");
    assert!(matches!(
        cleanup.status,
        PullRequestPreviewStatus::CleanupRequired {
            reason: PreviewCleanupReason::ForkDenied,
            requested_at,
        } if requested_at == timestamp(20)
    ));
    assert!(!cleanup.protected_secrets_eligible());
    let mut missing_fork_identity = cleanup.clone();
    missing_fork_identity.head_repository = None;
    assert!(missing_fork_identity.validate().is_err());

    let stale_fork = change(
        PullRequestChangeKind::Synchronized,
        timestamp(5),
        'c',
        Some(fork_repository()),
        false,
    );
    let unchanged = reconcile_pull_request_preview(&deny_policy, Some(&trusted), &stale_fork)
        .expect("stale fork observation");
    assert_eq!(unchanged.outcome, PreviewReconcileOutcome::IgnoredStale);
    assert_eq!(unchanged.preview, Some(trusted));
}

#[test]
fn reordered_open_sync_and_close_events_reach_one_logical_cleanup_state() {
    let policy = policy(PreviewForkPolicy::Isolated);
    let opened = change(
        PullRequestChangeKind::Opened,
        timestamp(10),
        'a',
        Some(base_repository()),
        false,
    );
    let synchronized = change(
        PullRequestChangeKind::Synchronized,
        timestamp(20),
        'b',
        Some(base_repository()),
        false,
    );
    let closed = change(
        PullRequestChangeKind::Closed,
        timestamp(30),
        'b',
        None,
        false,
    );

    let chronological = apply(&policy, [&opened, &synchronized, &closed]);
    let reversed = apply(&policy, [&closed, &synchronized, &opened]);
    assert_eq!(chronological.id, reversed.id);
    assert_eq!(chronological.environment_id, reversed.environment_id);
    assert_eq!(chronological.last_provider_updated_at, timestamp(30));
    assert_eq!(reversed.last_provider_updated_at, timestamp(30));
    assert_eq!(chronological.provider_created_at, timestamp(1));
    assert_eq!(reversed.provider_created_at, timestamp(1));
    assert_eq!(chronological.status, reversed.status);
    assert!(matches!(
        chronological.status,
        PullRequestPreviewStatus::CleanupRequired {
            reason: PreviewCleanupReason::PullRequestClosed,
            requested_at,
        } if requested_at == timestamp(30)
    ));
    assert_eq!(chronological.aggregate_version, 3);
    assert_eq!(reversed.aggregate_version, 1);
    let mut chronological_logical_state = chronological;
    let mut reversed_logical_state = reversed;
    chronological_logical_state.aggregate_version = 1;
    reversed_logical_state.aggregate_version = 1;
    assert_eq!(chronological_logical_state, reversed_logical_state);
}

#[test]
fn same_provider_timestamp_uses_a_total_content_order_independent_of_delivery_order() {
    let policy = policy(PreviewForkPolicy::Isolated);
    let lower = change(
        PullRequestChangeKind::Synchronized,
        timestamp(20),
        'a',
        Some(base_repository()),
        false,
    );
    let higher = change(
        PullRequestChangeKind::Synchronized,
        timestamp(20),
        'b',
        Some(base_repository()),
        false,
    );
    let lower_then_higher = apply(&policy, [&lower, &higher]);
    let higher_then_lower = apply(&policy, [&higher, &lower]);
    assert_eq!(lower_then_higher.head_commit_sha, higher.head_commit_sha);
    assert_eq!(higher_then_lower.head_commit_sha, higher.head_commit_sha);
    assert_eq!(lower_then_higher.id, higher_then_lower.id);
}

#[test]
fn same_provider_timestamp_gives_terminal_provider_state_precedence() {
    let policy = policy(PreviewForkPolicy::Isolated);
    let synchronized = change(
        PullRequestChangeKind::Synchronized,
        timestamp(20),
        'b',
        Some(base_repository()),
        false,
    );
    let closed = change(
        PullRequestChangeKind::Closed,
        timestamp(20),
        'a',
        None,
        false,
    );
    let synchronized_then_closed = apply(&policy, [&synchronized, &closed]);
    let closed_then_synchronized = apply(&policy, [&closed, &synchronized]);
    assert_eq!(
        synchronized_then_closed.status,
        closed_then_synchronized.status
    );
    assert!(matches!(
        synchronized_then_closed.status,
        PullRequestPreviewStatus::CleanupRequired {
            reason: PreviewCleanupReason::PullRequestClosed,
            requested_at,
        } if requested_at == timestamp(20)
    ));
}

#[test]
fn later_reopen_reuses_the_preview_identity_and_explicit_expiry_requests_cleanup() {
    let policy = policy(PreviewForkPolicy::Isolated);
    let opened = change(
        PullRequestChangeKind::Opened,
        timestamp(10),
        'a',
        Some(base_repository()),
        false,
    );
    let closed = change(
        PullRequestChangeKind::Closed,
        timestamp(20),
        'a',
        Some(base_repository()),
        true,
    );
    let reopened = change(
        PullRequestChangeKind::Reopened,
        timestamp(30),
        'c',
        Some(base_repository()),
        false,
    );
    let active = apply(&policy, [&opened]);
    let cleanup = reconcile_pull_request_preview(&policy, Some(&active), &closed)
        .expect("closed preview")
        .preview
        .expect("cleanup preview");
    assert!(matches!(
        cleanup.status,
        PullRequestPreviewStatus::CleanupRequired {
            reason: PreviewCleanupReason::PullRequestMerged,
            ..
        }
    ));
    let mut tampered_cleanup = cleanup.clone();
    tampered_cleanup.status = PullRequestPreviewStatus::CleanupRequired {
        reason: PreviewCleanupReason::PullRequestMerged,
        requested_at: timestamp(21),
    };
    assert!(tampered_cleanup.validate().is_err());
    let result = reconcile_pull_request_preview(&policy, Some(&cleanup), &reopened)
        .expect("reopened preview");
    assert_eq!(result.outcome, PreviewReconcileOutcome::Reactivated);
    let reactivated = result.preview.expect("reactivated preview");
    assert_eq!(reactivated.id, active.id);
    assert_eq!(reactivated.environment_id, active.environment_id);
    assert!(reactivated.status.is_active());

    assert!(reactivated
        .expire(reactivated.expires_at - TimeDelta::microseconds(1))
        .expect("early expiry decision")
        .is_none());
    let expired = reactivated
        .expire(reactivated.expires_at)
        .expect("expiry decision")
        .expect("expired preview");
    assert!(matches!(
        expired.status,
        PullRequestPreviewStatus::CleanupRequired {
            reason: PreviewCleanupReason::Expired,
            requested_at,
        } if requested_at == reactivated.expires_at
    ));
    assert_eq!(expired.aggregate_version, reactivated.aggregate_version + 1);

    let duplicate = reconcile_pull_request_preview(&policy, Some(&expired), &reopened)
        .expect("duplicate after expiry");
    assert_eq!(duplicate.outcome, PreviewReconcileOutcome::IgnoredDuplicate);
    assert_eq!(duplicate.preview, Some(expired));
}

#[test]
fn rejects_events_outside_the_exact_subscription_repository_and_branch_binding() {
    let policy = policy(PreviewForkPolicy::Isolated);
    let mut wrong_branch = change(
        PullRequestChangeKind::Opened,
        timestamp(10),
        'a',
        Some(base_repository()),
        false,
    );
    wrong_branch.base_reference = GitReference::parse("branch", "release").expect("branch");
    assert!(reconcile_pull_request_preview(&policy, None, &wrong_branch).is_err());

    let mut wrong_installation = wrong_branch;
    wrong_installation.base_reference = policy.base_reference.clone();
    wrong_installation.installation_id = GithubInstallationId::parse(43).expect("installation");
    assert!(reconcile_pull_request_preview(&policy, None, &wrong_installation).is_err());
}

fn apply<const N: usize>(
    policy: &PullRequestPreviewPolicy,
    changes: [&VerifiedPullRequestChange; N],
) -> PullRequestPreview {
    let mut current = None;
    for change in changes {
        current = reconcile_pull_request_preview(policy, current.as_ref(), change)
            .expect("preview reconciliation")
            .preview;
    }
    current.expect("preview state")
}

fn policy(fork_policy: PreviewForkPolicy) -> PullRequestPreviewPolicy {
    PullRequestPreviewPolicy {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        source_subscription_id: SourceSubscriptionId::new(),
        owner_principal_id: PrincipalId::new(),
        installation_id: GithubInstallationId::parse(42).expect("installation"),
        base_repository: base_repository(),
        base_reference: GitReference::parse("branch", "main").expect("base branch"),
        lifetime_seconds: 24 * 60 * 60,
        maximum_active_previews: 16,
        fork_policy,
        allow_protected_secrets_for_trusted_sources: true,
        quota: PreviewQuota {
            maximum_workloads: 4,
            cpu_millis: 4_000,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            ephemeral_storage_bytes: 16 * 1024 * 1024 * 1024,
        },
    }
}

fn change(
    kind: PullRequestChangeKind,
    provider_updated_at: DateTime<Utc>,
    sha_character: char,
    head_repository: Option<GitRepository>,
    merged: bool,
) -> VerifiedPullRequestChange {
    let value = VerifiedPullRequestChange {
        provider: GitProvider::Github,
        delivery_id: WebhookDeliveryId::parse(format!(
            "delivery-{}-{sha_character}",
            kind.as_str()
        ))
        .expect("delivery ID"),
        installation_id: GithubInstallationId::parse(42).expect("installation"),
        base_repository: base_repository(),
        base_reference: GitReference::parse("branch", "main").expect("base branch"),
        head_repository,
        head_reference: GitReference::parse("branch", "feature/preview").expect("head branch"),
        head_commit_sha: GitCommitSha::parse(sha_character.to_string().repeat(40))
            .expect("head commit"),
        pull_request_id: 1_000_042,
        pull_request_number: 42,
        kind,
        merged,
        provider_created_at: timestamp(1),
        provider_updated_at,
        payload_digest: format!("sha256:{}", sha_character.to_string().repeat(64)),
    };
    value.validate().expect("verified PR change");
    value
}

fn base_repository() -> GitRepository {
    GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")
        .expect("base repository")
}

fn fork_repository() -> GitRepository {
    GitRepository::parse(GitProvider::Github, "https://github.com/contributor/cloud")
        .expect("fork repository")
}

fn timestamp(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 24, 5, 30, second)
        .single()
        .expect("timestamp")
        .with_nanosecond(123_456_000)
        .expect("microseconds")
}
