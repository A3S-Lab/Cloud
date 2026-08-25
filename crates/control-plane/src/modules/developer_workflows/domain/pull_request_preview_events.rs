use super::{
    GitBranch, GithubInstallationRef, PreviewCleanupReason, PreviewForkPolicy, PreviewQuota,
    PullRequestChangeKind, PullRequestPreview, PullRequestPreviewPolicy,
    PullRequestPreviewPolicyAuthority, PullRequestPreviewStatus,
};
use crate::modules::developer_workflows::published::{
    PullRequestPreviewLifecycleCommitted, PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY,
    PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, GitCommitSha, SourcePullRequestChangeId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const MAX_PULL_REQUEST_PREVIEW_LIFECYCLE_BYTES: usize = 16 * 1024;

/// Domain translator between one validated Preview aggregate version and its
/// aggregate-free Published Language envelope.
pub struct PullRequestPreviewLifecycleEvent;

impl PullRequestPreviewLifecycleEvent {
    pub fn envelope(
        preview: &PullRequestPreview,
        source_pull_request_change_id: SourcePullRequestChangeId,
        occurred_at: DateTime<Utc>,
        correlation_id: Uuid,
        causation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        preview.validate()?;
        if source_pull_request_change_id.as_uuid().is_nil()
            || correlation_id.is_nil()
            || causation_id.is_nil()
            || occurred_at != canonical_timestamp(occurred_at)
        {
            return Err("Preview lifecycle event causality or time is invalid".into());
        }
        let payload = Self::from_preview(preview, source_pull_request_change_id);
        Self::validate_payload(&payload)?;
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY.into(),
            schema_version: PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
            organization_id: preview.policy_authority.policy.organization_id.as_uuid(),
            aggregate_id: preview.id.as_uuid(),
            aggregate_version: preview.aggregate_version,
            occurred_at,
            correlation_id,
            causation_id: Some(causation_id),
            payload: serde_json::to_value(payload)
                .map_err(|error| format!("Preview lifecycle event is invalid: {error}"))?,
        })
    }

    pub fn from_envelope(
        envelope: &DomainEventEnvelope,
    ) -> Result<PullRequestPreviewLifecycleCommitted, String> {
        if envelope.event_id.is_nil()
            || envelope.event_key != PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY
            || envelope.schema_version != PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION
            || envelope.occurred_at != canonical_timestamp(envelope.occurred_at)
            || envelope.correlation_id.is_nil()
            || envelope
                .causation_id
                .is_none_or(|causation_id| causation_id.is_nil())
        {
            return Err("Preview lifecycle envelope metadata is invalid".into());
        }
        canonical_json_bounded(
            &envelope.payload,
            MAX_PULL_REQUEST_PREVIEW_LIFECYCLE_BYTES,
            "Preview lifecycle payload",
        )?;
        let payload: PullRequestPreviewLifecycleCommitted =
            serde_json::from_value(envelope.payload.clone()).map_err(|error| {
                format!("Preview lifecycle payload could not be decoded: {error}")
            })?;
        Self::validate_payload(&payload)?;
        if envelope.organization_id != payload.organization_id.as_uuid()
            || envelope.aggregate_id != payload.preview_id.as_uuid()
            || envelope.aggregate_version != payload.preview_aggregate_version
        {
            return Err("Preview lifecycle envelope and payload identity differ".into());
        }
        Ok(payload)
    }

    pub fn validate_for(
        envelope: &DomainEventEnvelope,
        preview: &PullRequestPreview,
        source_pull_request_change_id: SourcePullRequestChangeId,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let payload = Self::from_envelope(envelope)?;
        if payload != Self::from_preview(preview, source_pull_request_change_id)
            || envelope.occurred_at != canonical_timestamp(occurred_at)
        {
            return Err("Preview lifecycle event changed its committed aggregate binding".into());
        }
        Ok(())
    }

    fn validate_payload(payload: &PullRequestPreviewLifecycleCommitted) -> Result<(), String> {
        if payload.source_pull_request_change_id.as_uuid().is_nil() {
            return Err("Preview lifecycle source fact identity is invalid".into());
        }
        let preview = Self::to_preview(payload)?;
        preview.validate()?;
        if payload != &Self::from_preview(&preview, payload.source_pull_request_change_id) {
            return Err("Preview lifecycle payload is not canonical".into());
        }
        Ok(())
    }

    fn from_preview(
        preview: &PullRequestPreview,
        source_pull_request_change_id: SourcePullRequestChangeId,
    ) -> PullRequestPreviewLifecycleCommitted {
        let authority = &preview.policy_authority;
        let policy = &authority.policy;
        let (head_repository_provider, head_repository_url, head_repository_identity) = preview
            .head_repository
            .as_ref()
            .map_or((None, None, None), |repository| {
                (
                    Some(repository.provider().as_str().into()),
                    Some(repository.canonical_url().into()),
                    Some(repository.identity().into()),
                )
            });
        let (status, cleanup_reason, cleanup_requested_at) = match &preview.status {
            PullRequestPreviewStatus::Active => ("active".into(), None, None),
            PullRequestPreviewStatus::CleanupRequired {
                reason,
                requested_at,
            } => (
                "cleanup_required".into(),
                Some(reason.as_str().into()),
                Some(*requested_at),
            ),
        };
        PullRequestPreviewLifecycleCommitted {
            source_pull_request_change_id,
            organization_id: policy.organization_id,
            project_id: policy.project_id,
            source_environment_id: authority.source_environment_id,
            source_subscription_id: policy.source_subscription_id,
            preview_policy_revision_id: authority.revision_id,
            preview_policy_revision_number: authority.revision_number,
            preview_policy_accepted_at: authority.accepted_at,
            preview_id: preview.id,
            preview_aggregate_version: preview.aggregate_version,
            environment_id: preview.environment_id,
            environment_name: preview.environment_name(),
            owner_principal_id: policy.owner_principal_id,
            installation_id: policy.installation_id.as_u64(),
            base_repository_provider: policy.base_repository.provider().as_str().into(),
            base_repository_url: policy.base_repository.canonical_url().into(),
            base_repository_identity: policy.base_repository.identity().into(),
            base_branch: policy.base_branch.as_str().into(),
            head_repository_provider,
            head_repository_url,
            head_repository_identity,
            head_branch: preview.head_branch.as_str().into(),
            head_commit_sha: preview.head_commit_sha.as_str().into(),
            pull_request_id: preview.pull_request_id,
            pull_request_number: preview.pull_request_number,
            provider_created_at: preview.provider_created_at,
            last_provider_updated_at: preview.last_provider_updated_at,
            last_change_kind: preview.last_change_kind.as_str().into(),
            last_merged: preview.last_merged,
            expires_at: preview.expires_at,
            status,
            cleanup_reason,
            cleanup_requested_at,
            fork_policy: policy.fork_policy.as_str().into(),
            is_fork: preview.is_fork(),
            allow_protected_secrets_for_trusted_sources: policy
                .allow_protected_secrets_for_trusted_sources,
            protected_secrets_eligible: preview.protected_secrets_eligible(),
            lifetime_seconds: policy.lifetime_seconds,
            maximum_active_previews: policy.maximum_active_previews,
            maximum_workloads: policy.quota.maximum_workloads,
            cpu_millis: policy.quota.cpu_millis,
            memory_bytes: policy.quota.memory_bytes,
            ephemeral_storage_bytes: policy.quota.ephemeral_storage_bytes,
        }
    }

    fn to_preview(
        payload: &PullRequestPreviewLifecycleCommitted,
    ) -> Result<PullRequestPreview, String> {
        let base_repository = GitRepository::parse(
            GitProvider::parse(&payload.base_repository_provider)?,
            &payload.base_repository_url,
        )?;
        if base_repository.identity() != payload.base_repository_identity {
            return Err("Preview lifecycle base repository identity drifted".into());
        }
        let head_repository = match (
            payload.head_repository_provider.as_deref(),
            payload.head_repository_url.as_deref(),
            payload.head_repository_identity.as_deref(),
        ) {
            (None, None, None) => None,
            (Some(provider), Some(url), Some(identity)) => {
                let repository = GitRepository::parse(GitProvider::parse(provider)?, url)?;
                if repository.identity() != identity {
                    return Err("Preview lifecycle head repository identity drifted".into());
                }
                Some(repository)
            }
            _ => return Err("Preview lifecycle head repository binding is incomplete".into()),
        };
        let status = match (
            payload.status.as_str(),
            payload.cleanup_reason.as_deref(),
            payload.cleanup_requested_at,
        ) {
            ("active", None, None) => PullRequestPreviewStatus::Active,
            ("cleanup_required", Some(reason), Some(requested_at)) => {
                PullRequestPreviewStatus::CleanupRequired {
                    reason: PreviewCleanupReason::parse(reason)?,
                    requested_at,
                }
            }
            _ => return Err("Preview lifecycle status evidence is invalid".into()),
        };
        PullRequestPreview::restore(PullRequestPreview {
            policy_authority: PullRequestPreviewPolicyAuthority {
                source_environment_id: payload.source_environment_id,
                revision_id: payload.preview_policy_revision_id,
                revision_number: payload.preview_policy_revision_number,
                accepted_at: payload.preview_policy_accepted_at,
                policy: PullRequestPreviewPolicy {
                    organization_id: payload.organization_id,
                    project_id: payload.project_id,
                    source_subscription_id: payload.source_subscription_id,
                    owner_principal_id: payload.owner_principal_id,
                    installation_id: GithubInstallationRef::parse(payload.installation_id)?,
                    base_repository,
                    base_branch: GitBranch::parse(&payload.base_branch)?,
                    lifetime_seconds: payload.lifetime_seconds,
                    maximum_active_previews: payload.maximum_active_previews,
                    fork_policy: PreviewForkPolicy::parse(&payload.fork_policy)?,
                    allow_protected_secrets_for_trusted_sources: payload
                        .allow_protected_secrets_for_trusted_sources,
                    quota: PreviewQuota {
                        maximum_workloads: payload.maximum_workloads,
                        cpu_millis: payload.cpu_millis,
                        memory_bytes: payload.memory_bytes,
                        ephemeral_storage_bytes: payload.ephemeral_storage_bytes,
                    },
                },
            },
            id: payload.preview_id,
            environment_id: payload.environment_id,
            pull_request_id: payload.pull_request_id,
            pull_request_number: payload.pull_request_number,
            head_repository,
            head_branch: GitBranch::parse(&payload.head_branch)?,
            head_commit_sha: GitCommitSha::parse(&payload.head_commit_sha)?,
            provider_created_at: payload.provider_created_at,
            last_provider_updated_at: payload.last_provider_updated_at,
            last_change_kind: PullRequestChangeKind::parse(&payload.last_change_kind)?,
            last_merged: payload.last_merged,
            expires_at: payload.expires_at,
            status,
            aggregate_version: payload.preview_aggregate_version,
        })
    }
}
