use super::{
    AppendAgentExecutionEvents, AppendAgentExecutionEventsHandler, CreateAgentConversation,
    CreateAgentConversationHandler, GetAgentExecutionEvents, GetAgentExecutionEventsHandler,
    StartAgentExecution, StartAgentExecutionHandler,
};
use crate::modules::agents::domain::{
    AgentEventContent, AgentExecutionEventDraft, AgentExecutionEventKind, AgentExecutionStatus,
};
use crate::modules::agents::InMemoryAgentRepository;
use crate::modules::artifacts::domain::test_support::succeeded_hosted_build;
use crate::modules::artifacts::InMemoryBuildRunRepository;
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseVersion, AssetReleaseWrite, AssetWrite,
    CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository, TransitionAssetReleaseWrite,
    TransitionAssetWrite,
};
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, EnvironmentId, GitCommitSha, IdempotencyRequest,
    IdempotentWrite, OrganizationId, ProjectId, RepositoryError, ResourceName, Sha256Digest,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn conversation_execution_and_semantic_events_are_replayable_end_to_end() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let drafted_at = canonical_timestamp(Utc::now() - Duration::minutes(1));
    let requested_at = drafted_at + Duration::seconds(1);
    let environment = Environment::create(
        organization_id,
        project_id,
        environment_id,
        EnvironmentName::parse("Production").expect("Environment name"),
        drafted_at,
    );
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("research-agent").expect("Asset name"),
        AssetKind::Agent,
        drafted_at,
    )
    .expect("Agent Asset");
    let (release, build) = published_release(&asset, drafted_at);
    let environments = Arc::new(TestEnvironmentRepository { environment });
    let assets = Arc::new(TestAssetRepository {
        asset: asset.clone(),
        release: release.clone(),
    });
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    builds.seed_build(build).await;
    let agents = Arc::new(InMemoryAgentRepository::new());
    let context = || CqrsContext::new(ModuleRef::new());

    let create_handler = CreateAgentConversationHandler::new(environments, agents.clone());
    let create = CreateAgentConversation {
        organization_id,
        project_id,
        environment_id,
        idempotency_key: "agent-conversation:create".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    };
    let created = create_handler
        .execute(create.clone(), context())
        .await
        .expect("create handler")
        .expect("create conversation");
    let replayed_conversation = create_handler
        .execute(create, context())
        .await
        .expect("create replay handler")
        .expect("replay conversation");
    assert!(replayed_conversation.replayed);
    assert_eq!(
        replayed_conversation.conversation.id,
        created.conversation.id
    );

    let start_handler = StartAgentExecutionHandler::new(agents.clone(), assets, builds);
    let start = StartAgentExecution {
        organization_id,
        conversation_id: created.conversation.id,
        agent_asset_id: asset.id,
        agent_asset_release_id: release.id,
        input: serde_json::json!({"message": "hello"}),
        idempotency_key: "agent-execution:start".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    };
    let started = start_handler
        .execute(start.clone(), context())
        .await
        .expect("start handler")
        .expect("start execution");
    assert_eq!(started.conversation.last_event_sequence, 1);
    assert_eq!(started.execution.status, AgentExecutionStatus::Pending);
    let replayed_execution = start_handler
        .execute(start, context())
        .await
        .expect("start replay handler")
        .expect("replay execution");
    assert!(replayed_execution.replayed);
    assert_eq!(replayed_execution.execution.id, started.execution.id);

    let event_at = requested_at + Duration::seconds(1);
    let append_handler = AppendAgentExecutionEventsHandler::new(agents.clone());
    let append = AppendAgentExecutionEvents {
        organization_id,
        conversation_id: created.conversation.id,
        execution_id: started.execution.id,
        events: vec![
            event(
                AgentExecutionEventKind::ModelOutput,
                serde_json::json!({"text": "hello"}),
                event_at,
            ),
            event(
                AgentExecutionEventKind::ExecutionCompleted,
                serde_json::json!({}),
                event_at,
            ),
        ],
        idempotency_key: "agent-execution:events:complete".into(),
    };
    let appended = append_handler
        .execute(append.clone(), context())
        .await
        .expect("append handler")
        .expect("append events");
    assert_eq!(appended.conversation.last_event_sequence, 3);
    assert_eq!(appended.execution.status, AgentExecutionStatus::Succeeded);
    assert_eq!(appended.events.len(), 2);
    let replayed_events = append_handler
        .execute(append, context())
        .await
        .expect("append replay handler")
        .expect("replay events");
    assert!(replayed_events.replayed);
    assert_eq!(replayed_events.events, appended.events);

    let page = GetAgentExecutionEventsHandler::new(agents)
        .execute(
            GetAgentExecutionEvents {
                organization_id,
                conversation_id: created.conversation.id,
                after_sequence: None,
                limit: 10,
            },
            context(),
        )
        .await
        .expect("event query handler")
        .expect("query events");
    assert_eq!(page.head_sequence, 3);
    assert_eq!(page.records.len(), 3);
    assert_eq!(
        page.records[0].kind,
        AgentExecutionEventKind::ExecutionRequested
    );
    assert_eq!(
        page.records[2].kind,
        AgentExecutionEventKind::ExecutionCompleted
    );
}

fn event(
    kind: AgentExecutionEventKind,
    content: serde_json::Value,
    occurred_at: chrono::DateTime<Utc>,
) -> AgentExecutionEventDraft {
    AgentExecutionEventDraft::new(
        kind,
        AgentEventContent::inline_json(content).expect("event content"),
        occurred_at,
    )
    .expect("event draft")
}

fn published_release(
    asset: &Asset,
    drafted_at: chrono::DateTime<Utc>,
) -> (AssetRelease, crate::modules::artifacts::BuildRun) {
    let mut release = AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0").expect("release version"),
        GitCommitSha::parse("a".repeat(40)).expect("commit SHA"),
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("manifest digest"),
        drafted_at,
    )
    .expect("draft release");
    let build = succeeded_hosted_build(asset.organization_id, asset.id, release.id, drafted_at);
    release
        .publish_from_build(asset, &build)
        .expect("publish release");
    (release, build)
}

struct TestEnvironmentRepository {
    environment: Environment,
}

#[async_trait]
impl IEnvironmentRepository for TestEnvironmentRepository {
    async fn create(
        &self,
        _environment: Environment,
        _event: DomainEventEnvelope,
        _idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        Err(RepositoryError::Storage("unused Environment write".into()))
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        Ok((self.environment.organization_id == organization_id
            && self.environment.project_id == project_id
            && self.environment.id == environment_id)
            .then(|| self.environment.clone()))
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        Ok((self.environment.organization_id == organization_id
            && self.environment.project_id == project_id)
            .then(|| self.environment.clone())
            .into_iter()
            .collect())
    }
}

struct TestAssetRepository {
    asset: Asset,
    release: AssetRelease,
}

#[async_trait]
impl IAssetRepository for TestAssetRepository {
    async fn create_asset(&self, _write: CreateAssetWrite) -> Result<AssetWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused Asset write".into()))
    }

    async fn transition_asset(
        &self,
        _write: TransitionAssetWrite,
    ) -> Result<AssetWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused Asset transition".into()))
    }

    async fn find_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Option<Asset>, RepositoryError> {
        Ok(
            (self.asset.organization_id == organization_id && self.asset.id == asset_id)
                .then(|| self.asset.clone()),
        )
    }

    async fn list_assets(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<Asset>, RepositoryError> {
        Ok((self.asset.organization_id == organization_id)
            .then(|| self.asset.clone())
            .into_iter()
            .collect())
    }

    async fn create_release(
        &self,
        _write: CreateAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused release write".into()))
    }

    async fn transition_release(
        &self,
        _write: TransitionAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused release transition".into()))
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<AssetRelease>, RepositoryError> {
        Ok((self.asset.organization_id == organization_id
            && self.asset.id == asset_id
            && self.release.id == asset_release_id)
            .then(|| self.release.clone()))
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Vec<AssetRelease>, RepositoryError> {
        Ok(
            (self.asset.organization_id == organization_id && self.asset.id == asset_id)
                .then(|| self.release.clone())
                .into_iter()
                .collect(),
        )
    }
}
