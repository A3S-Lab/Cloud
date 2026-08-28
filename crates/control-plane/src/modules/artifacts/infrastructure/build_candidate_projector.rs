use crate::modules::artifacts::application::{
    BuildCandidate, BuildCandidateEvidence, IArtifactBuildProjectionPort,
    PreviewBuildLifecycleState, PreviewBuildSourceRevision, ProjectPreviewBuildLifecycle,
};
use crate::modules::artifacts::domain::BuildSubject;
use crate::modules::assets::published::{
    HostedAssetBuildRequestedFact, HOSTED_ASSET_BUILD_REQUESTED_EVENT_KEY,
    HOSTED_ASSET_BUILD_REQUESTED_SCHEMA_VERSION,
};
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, GitCommitSha, RepositoryError, Sha256Digest,
};
use crate::modules::sources::published::{
    PreviewSourceRevisionLifecycleCommittedFact, PreviewSourceRevisionLifecycleState,
    SourceRevisionAcceptedFact, PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_MAX_BYTES, SOURCE_REVISION_ACCEPTED_EVENT_KEY,
    SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from owner-published integration facts into the
/// Artifacts candidate projection.
pub struct BuildCandidateProjector {
    projections: Arc<dyn IArtifactBuildProjectionPort>,
}

impl BuildCandidateProjector {
    pub fn new(projections: Arc<dyn IArtifactBuildProjectionPort>) -> Self {
        Self { projections }
    }

    async fn project_source(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        let fact: SourceRevisionAcceptedFact = decode(message, "accepted Source revision")?;
        fact.validate().map_err(invalid_message)?;
        if message.schema_version != SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION
            || message.organization_id() != Some(fact.organization_id().as_uuid())
            || message.aggregate_id != fact.source_revision_id().as_uuid()
            || message.aggregate_version != 1
            || message.correlation_id.is_nil()
            || message
                .causation_id
                .is_some_and(|causation_id| causation_id.is_nil())
        {
            return Err(invalid_message(
                "accepted Source revision envelope and fact identity differ".into(),
            ));
        }
        let evidence = BuildCandidateEvidence::external_source_revision(
            fact.repository_identity().to_owned(),
            GitCommitSha::parse(fact.commit_sha()).map_err(invalid_message)?,
            Sha256Digest::parse(fact.recipe_digest()).map_err(invalid_message)?,
        )
        .map_err(invalid_message)?;
        let candidate = BuildCandidate::new(
            fact.organization_id(),
            BuildSubject::external_source_revision(
                fact.project_id(),
                fact.environment_id(),
                fact.source_revision_id(),
            ),
            evidence,
            message.occurred_at,
        )
        .map_err(invalid_message)?;
        self.projections.project_candidate(candidate).await
    }

    async fn project_hosted_asset(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        let fact: HostedAssetBuildRequestedFact = decode(message, "hosted Asset build request")?;
        fact.validate().map_err(invalid_message)?;
        if message.schema_version != HOSTED_ASSET_BUILD_REQUESTED_SCHEMA_VERSION
            || message.organization_id() != Some(fact.organization_id().as_uuid())
            || message.aggregate_id != fact.asset_release_id().as_uuid()
            || message.aggregate_version != 1
            || message.correlation_id.is_nil()
            || message
                .causation_id
                .is_some_and(|causation_id| causation_id.is_nil())
        {
            return Err(invalid_message(
                "hosted Asset build request envelope and fact identity differ".into(),
            ));
        }
        let evidence = BuildCandidateEvidence::hosted_asset_release(
            GitCommitSha::parse(fact.commit_sha()).map_err(invalid_message)?,
            Sha256Digest::parse(fact.manifest_digest()).map_err(invalid_message)?,
        );
        let candidate = BuildCandidate::new(
            fact.organization_id(),
            BuildSubject::asset_release(fact.asset_id(), fact.asset_release_id()),
            evidence,
            message.occurred_at,
        )
        .map_err(invalid_message)?;
        self.projections.project_candidate(candidate).await
    }

    async fn project_preview_source_lifecycle(
        &self,
        message: &OutboxMessage,
    ) -> Result<(), RepositoryError> {
        canonical_json_bounded(
            &message.payload,
            PREVIEW_SOURCE_REVISION_LIFECYCLE_MAX_BYTES,
            "Preview SourceRevision lifecycle fact",
        )
        .map_err(invalid_message)?;
        let fact: PreviewSourceRevisionLifecycleCommittedFact =
            decode(message, "Preview SourceRevision lifecycle")?;
        fact.validate().map_err(invalid_message)?;
        let lifecycle_causation_id = message.causation_id.ok_or_else(|| {
            invalid_message("Preview SourceRevision lifecycle has no causation".into())
        })?;
        if message.event_id.is_nil()
            || message.schema_version != PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION
            || message.organization_id() != Some(fact.organization_id().as_uuid())
            || message.aggregate_id != fact.preview_id().as_uuid()
            || message.aggregate_version != fact.preview_aggregate_version()
            || message.occurred_at != canonical_timestamp(message.occurred_at)
            || message.correlation_id.is_nil()
            || lifecycle_causation_id.is_nil()
        {
            return Err(invalid_message(
                "Preview SourceRevision lifecycle envelope and fact identity differ".into(),
            ));
        }
        let state = match fact.state() {
            PreviewSourceRevisionLifecycleState::Active => PreviewBuildLifecycleState::Active,
            PreviewSourceRevisionLifecycleState::CleanupRequired => {
                PreviewBuildLifecycleState::CleanupRequired
            }
            PreviewSourceRevisionLifecycleState::SuppressedInactiveSubscription => {
                PreviewBuildLifecycleState::SuppressedInactiveSubscription
            }
        };
        let source_revision = match fact.state() {
            PreviewSourceRevisionLifecycleState::Active => Some(PreviewBuildSourceRevision {
                source_revision_id: fact.source_revision_id().ok_or_else(|| {
                    invalid_message(
                        "active Preview lifecycle omitted its SourceRevision identity".into(),
                    )
                })?,
                repository_identity: fact
                    .repository_identity()
                    .ok_or_else(|| {
                        invalid_message(
                            "active Preview lifecycle omitted its repository identity".into(),
                        )
                    })?
                    .to_owned(),
                commit_sha: GitCommitSha::parse(fact.commit_sha().ok_or_else(|| {
                    invalid_message("active Preview lifecycle omitted its commit".into())
                })?)
                .map_err(invalid_message)?,
                recipe_digest: Sha256Digest::parse(fact.recipe_digest().ok_or_else(|| {
                    invalid_message("active Preview lifecycle omitted its recipe".into())
                })?)
                .map_err(invalid_message)?,
                accepted_at: fact.source_revision_accepted_at().ok_or_else(|| {
                    invalid_message(
                        "active Preview lifecycle omitted its SourceRevision acceptance time"
                            .into(),
                    )
                })?,
            }),
            PreviewSourceRevisionLifecycleState::CleanupRequired
            | PreviewSourceRevisionLifecycleState::SuppressedInactiveSubscription => None,
        };
        let input = ProjectPreviewBuildLifecycle {
            lifecycle_event_id: message.event_id,
            correlation_id: message.correlation_id,
            lifecycle_causation_id,
            source_pull_request_change_id: fact.source_pull_request_change_id(),
            organization_id: fact.organization_id(),
            project_id: fact.project_id(),
            source_environment_id: fact.source_environment_id(),
            source_subscription_id: fact.source_subscription_id(),
            preview_id: fact.preview_id(),
            preview_aggregate_version: fact.preview_aggregate_version(),
            preview_environment_id: fact.preview_environment_id(),
            state,
            source_revision,
            fact_occurred_at: message.occurred_at,
        };
        input.validate().map_err(invalid_message)?;
        self.projections
            .project_preview_build_lifecycle(input)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl IIntegrationEventProjector for BuildCandidateProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        match message.event_key.as_str() {
            SOURCE_REVISION_ACCEPTED_EVENT_KEY => self.project_source(message).await,
            HOSTED_ASSET_BUILD_REQUESTED_EVENT_KEY => self.project_hosted_asset(message).await,
            PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY => {
                self.project_preview_source_lifecycle(message).await
            }
            _ => Ok(()),
        }
    }
}

fn decode<T>(message: &OutboxMessage, label: &str) -> Result<T, RepositoryError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(message.payload.clone())
        .map_err(|error| invalid_message(format!("{label} payload could not be decoded: {error}")))
}

fn invalid_message(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "Artifacts build projection fact is invalid: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::{BuildRunStatus, IBuildRunRepository};
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::assets::domain::{
        Asset, AssetKind, AssetRelease, AssetReleaseVersion, HostedAssetBuildRequested,
    };
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, EnvironmentId, GitCommitSha, OrganizationId, ProjectId,
        PullRequestPreviewId, ResourceName, Sha256Digest, SourcePullRequestChangeId,
        SourceRevisionId, SourceSubscriptionId,
    };
    use crate::modules::sources::domain::{
        ExternalSourceRevision, NewExternalSourceRevision, SourceRevisionAccepted,
    };
    use crate::modules::sources::published::{BuildRecipe, GitProvider, GitRepository};
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn source_fact_projects_once_and_reservation_reads_only_local_material() {
        let requested_at = Utc::now();
        let revision = source_revision(requested_at);
        let envelope =
            SourceRevisionAccepted::envelope(&revision, Uuid::now_v7()).expect("source fact");
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let projections: Arc<dyn IArtifactBuildProjectionPort> = repository.clone();
        let projector = BuildCandidateProjector::new(projections);
        let message = outbox_message(envelope);

        projector.project(&message).await.expect("first projection");
        projector
            .project(&message)
            .await
            .expect("idempotent replay");
        let mut conflicting = message.clone();
        conflicting.event_id = Uuid::now_v7();
        conflicting.payload["commit_sha"] = serde_json::json!("d".repeat(40));
        assert!(matches!(
            projector.project(&conflicting).await,
            Err(RepositoryError::Conflict(message))
                if message.contains("conflicts with its existing projection")
        ));
        let builds = repository
            .reserve_pending(10)
            .await
            .expect("reserve projected candidate");

        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].source_revision_id(), Some(revision.id));
        assert_eq!(builds[0].requested_at, revision.accepted_at);
    }

    #[tokio::test]
    async fn hosted_request_projects_exact_asset_release_and_rejects_envelope_drift() {
        let requested_at = Utc::now();
        let asset = Asset::create(
            AssetId::new(),
            OrganizationId::new(),
            ResourceName::parse("projected-agent").expect("name"),
            AssetKind::Agent,
            requested_at,
        )
        .expect("Asset");
        let release = AssetRelease::draft(
            &asset,
            AssetReleaseId::new(),
            AssetReleaseVersion::parse("1.0.0").expect("version"),
            GitCommitSha::parse("a".repeat(40)).expect("commit"),
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("digest"),
            requested_at,
        )
        .expect("release");
        let envelope = HostedAssetBuildRequested::envelope(&asset, &release, Uuid::now_v7())
            .expect("hosted build request");
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let projections: Arc<dyn IArtifactBuildProjectionPort> = repository.clone();
        let projector = BuildCandidateProjector::new(projections);

        projector
            .project(&outbox_message(envelope.clone()))
            .await
            .expect("hosted projection");
        let build = repository
            .reserve_pending(1)
            .await
            .expect("reserve hosted build")
            .pop()
            .expect("one hosted build");
        assert_eq!(build.asset_id(), Some(asset.id));
        assert_eq!(build.asset_release_id(), Some(release.id));

        let mut conflicting = outbox_message(envelope.clone());
        conflicting.event_id = Uuid::now_v7();
        conflicting.payload["manifest_digest"] =
            serde_json::json!(format!("sha256:{}", "e".repeat(64)));
        assert!(matches!(
            projector.project(&conflicting).await,
            Err(RepositoryError::Conflict(message))
                if message.contains("conflicts with its existing projection")
        ));

        let mut drifted = outbox_message(envelope);
        drifted.aggregate_id = Uuid::now_v7();
        assert!(matches!(
            projector.project(&drifted).await,
            Err(RepositoryError::Storage(message))
                if message.contains("envelope and fact identity differ")
        ));
    }

    #[tokio::test]
    async fn preview_lifecycle_fact_admits_one_build_and_cancels_it_on_cleanup() {
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let projections: Arc<dyn IArtifactBuildProjectionPort> = repository.clone();
        let projector = BuildCandidateProjector::new(projections);
        let fixture = PreviewFactFixture::new();
        let source_revision_id = SourceRevisionId::new();
        let accepted_at = fixture.base_at;
        let active = fixture.message(
            1,
            PreviewSourceRevisionLifecycleState::Active,
            Some((source_revision_id, accepted_at, 'a')),
        );

        projector.project(&active).await.expect("active projection");
        projector
            .project(&active)
            .await
            .expect("exact lifecycle replay");
        let build = repository
            .reserve_pending(2)
            .await
            .expect("reserve Preview build")
            .pop()
            .expect("one Preview build");
        assert_eq!(build.source_revision_id(), Some(source_revision_id));
        assert_eq!(build.requested_at, accepted_at);
        assert!(repository
            .reserve_pending(1)
            .await
            .expect("repeat reservation")
            .is_empty());

        let cleanup = fixture.message(
            2,
            PreviewSourceRevisionLifecycleState::CleanupRequired,
            None,
        );
        projector
            .project(&cleanup)
            .await
            .expect("cleanup projection");
        let cancelling = repository
            .find(fixture.organization_id, build.id)
            .await
            .expect("retired Preview build");
        assert_eq!(cancelling.status, BuildRunStatus::Cancelling);
        assert!(repository
            .reserve_pending(1)
            .await
            .expect("cleanup suppresses reservation")
            .is_empty());
    }

    #[tokio::test]
    async fn preview_lifecycle_fact_rejects_unbound_or_oversized_envelopes() {
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let projections: Arc<dyn IArtifactBuildProjectionPort> = repository;
        let projector = BuildCandidateProjector::new(projections);
        let fixture = PreviewFactFixture::new();
        let source_revision_id = SourceRevisionId::new();
        let active = fixture.message(
            1,
            PreviewSourceRevisionLifecycleState::Active,
            Some((source_revision_id, fixture.base_at, 'b')),
        );

        let mut missing_causation = active.clone();
        missing_causation.causation_id = None;
        assert!(matches!(
            projector.project(&missing_causation).await,
            Err(RepositoryError::Storage(message)) if message.contains("has no causation")
        ));

        let mut drifted_version = active.clone();
        drifted_version.aggregate_version += 1;
        assert!(matches!(
            projector.project(&drifted_version).await,
            Err(RepositoryError::Storage(message))
                if message.contains("envelope and fact identity differ")
        ));

        let mut oversized = active;
        oversized.payload["untrusted_padding"] =
            serde_json::json!("x".repeat(PREVIEW_SOURCE_REVISION_LIFECYCLE_MAX_BYTES));
        assert!(matches!(
            projector.project(&oversized).await,
            Err(RepositoryError::Storage(message)) if message.contains("exceeds")
        ));
    }

    struct PreviewFactFixture {
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        preview_id: PullRequestPreviewId,
        preview_environment_id: EnvironmentId,
        correlation_id: Uuid,
        base_at: chrono::DateTime<Utc>,
    }

    impl PreviewFactFixture {
        fn new() -> Self {
            Self {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                source_environment_id: EnvironmentId::new(),
                source_subscription_id: SourceSubscriptionId::new(),
                preview_id: PullRequestPreviewId::new(),
                preview_environment_id: EnvironmentId::new(),
                correlation_id: Uuid::now_v7(),
                base_at: canonical_timestamp(Utc::now()),
            }
        }

        fn message(
            &self,
            version: u64,
            state: PreviewSourceRevisionLifecycleState,
            revision: Option<(SourceRevisionId, chrono::DateTime<Utc>, char)>,
        ) -> OutboxMessage {
            let (source_revision_id, repository_identity, commit_sha, recipe_digest, accepted_at) =
                match revision {
                    Some((source_revision_id, accepted_at, fill)) => (
                        Some(source_revision_id),
                        Some("github:github.com/a3s-lab/cloud"),
                        Some(fill.to_string().repeat(40)),
                        Some(format!("sha256:{}", fill.to_string().repeat(64))),
                        Some(accepted_at),
                    ),
                    None => (None, None, None, None, None),
                };
            let occurred_at = self.base_at + chrono::Duration::seconds(version as i64);
            OutboxMessage {
                event_id: Uuid::now_v7(),
                event_key: PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY.into(),
                schema_version: PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
                scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
                    crate::modules::shared_kernel::domain::InstallationId::new(),
                    crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                        self.organization_id.as_uuid(),
                    ),
                )
                .expect("scope"),
                aggregate_id: self.preview_id.as_uuid(),
                aggregate_version: version,
                occurred_at,
                correlation_id: self.correlation_id,
                causation_id: Some(Uuid::now_v7()),
                payload: serde_json::json!({
                    "source_pull_request_change_id": SourcePullRequestChangeId::new(),
                    "organization_id": self.organization_id,
                    "project_id": self.project_id,
                    "source_environment_id": self.source_environment_id,
                    "source_subscription_id": self.source_subscription_id,
                    "preview_id": self.preview_id,
                    "preview_aggregate_version": version,
                    "preview_environment_id": self.preview_environment_id,
                    "state": state.as_str(),
                    "source_revision_id": source_revision_id,
                    "repository_identity": repository_identity,
                    "commit_sha": commit_sha,
                    "recipe_digest": recipe_digest,
                    "source_revision_accepted_at": accepted_at,
                }),
                delivery_attempts: 1,
            }
        }
    }

    fn source_revision(accepted_at: chrono::DateTime<Utc>) -> ExternalSourceRevision {
        ExternalSourceRevision::accept(NewExternalSourceRevision {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            id: SourceRevisionId::new(),
            repository: GitRepository::parse(
                GitProvider::Github,
                "https://github.com/A3S-Lab/Cloud.git",
            )
            .expect("repository"),
            commit_sha: GitCommitSha::parse("c".repeat(40)).expect("commit"),
            recipe: BuildRecipe::dockerfile(
                BuildRecipe::SCHEMA,
                BuildRecipe::DOCKERFILE_KIND,
                ".",
                "Dockerfile",
                None,
                vec!["linux/amd64".into()],
            )
            .expect("recipe"),
            accepted_at,
        })
        .expect("revision")
    }

    fn outbox_message(event: a3s_cloud_contracts::DomainEventEnvelope) -> OutboxMessage {
        let event_organization_id = event.organization_id().expect("tenant event");
        OutboxMessage {
            event_id: event.event_id,
            event_key: event.event_key,
            schema_version: event.schema_version,
            scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
                crate::modules::shared_kernel::domain::InstallationId::new(),
                crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                    event_organization_id,
                ),
            )
            .expect("scope"),
            aggregate_id: event.aggregate_id,
            aggregate_version: event.aggregate_version,
            occurred_at: event.occurred_at,
            correlation_id: event.correlation_id,
            causation_id: event.causation_id,
            payload: event.payload,
            delivery_attempts: 1,
        }
    }
}
