use crate::modules::artifacts::application::{
    BuildCandidate, BuildCandidateEvidence, IBuildCandidateProjectionPort,
};
use crate::modules::artifacts::domain::BuildSubject;
use crate::modules::assets::published::{
    HostedAssetBuildRequestedFact, HOSTED_ASSET_BUILD_REQUESTED_EVENT_KEY,
    HOSTED_ASSET_BUILD_REQUESTED_SCHEMA_VERSION,
};
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::shared_kernel::domain::{GitCommitSha, RepositoryError, Sha256Digest};
use crate::modules::sources::published::{
    SourceRevisionAcceptedFact, SOURCE_REVISION_ACCEPTED_EVENT_KEY,
    SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from owner-published integration facts into the
/// Artifacts candidate projection.
pub struct BuildCandidateProjector {
    candidates: Arc<dyn IBuildCandidateProjectionPort>,
}

impl BuildCandidateProjector {
    pub fn new(candidates: Arc<dyn IBuildCandidateProjectionPort>) -> Self {
        Self { candidates }
    }

    async fn project_source(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        let fact: SourceRevisionAcceptedFact = decode(message, "accepted Source revision")?;
        fact.validate().map_err(invalid_message)?;
        if message.schema_version != SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION
            || message.organization_id != fact.organization_id().as_uuid()
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
        self.candidates.project_candidate(candidate).await
    }

    async fn project_hosted_asset(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        let fact: HostedAssetBuildRequestedFact = decode(message, "hosted Asset build request")?;
        fact.validate().map_err(invalid_message)?;
        if message.schema_version != HOSTED_ASSET_BUILD_REQUESTED_SCHEMA_VERSION
            || message.organization_id != fact.organization_id().as_uuid()
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
        self.candidates.project_candidate(candidate).await
    }
}

#[async_trait]
impl IIntegrationEventProjector for BuildCandidateProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        match message.event_key.as_str() {
            SOURCE_REVISION_ACCEPTED_EVENT_KEY => self.project_source(message).await,
            HOSTED_ASSET_BUILD_REQUESTED_EVENT_KEY => self.project_hosted_asset(message).await,
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
        "Artifacts build candidate fact is invalid: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::IBuildRunRepository;
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::assets::domain::{
        Asset, AssetKind, AssetRelease, AssetReleaseVersion, HostedAssetBuildRequested,
    };
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, EnvironmentId, GitCommitSha, OrganizationId, ProjectId,
        ResourceName, Sha256Digest, SourceRevisionId,
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
        let candidates: Arc<dyn IBuildCandidateProjectionPort> = repository.clone();
        let projector = BuildCandidateProjector::new(candidates);
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
        let candidates: Arc<dyn IBuildCandidateProjectionPort> = repository.clone();
        let projector = BuildCandidateProjector::new(candidates);

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
        OutboxMessage {
            event_id: event.event_id,
            event_key: event.event_key,
            schema_version: event.schema_version,
            organization_id: event.organization_id,
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
