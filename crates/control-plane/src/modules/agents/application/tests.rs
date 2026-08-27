use super::{
    AgentExecutionReconciler, AppendAgentExecutionEvents, AppendAgentExecutionEventsHandler,
    CancelAgentExecution, CancelAgentExecutionHandler, CreateAgentConversation,
    CreateAgentConversationHandler, DecideAgentApprovalCheckpoint,
    DecideAgentApprovalCheckpointHandler, GetAgentExecutionEvents, GetAgentExecutionEventsHandler,
    IWorkflowAgentPort, ListAgentApprovalCheckpoints, ListAgentApprovalCheckpointsHandler,
    StartAgentExecution, StartAgentExecutionHandler, WorkflowAgentApplicationService,
    WorkflowAgentRequest, AGENT_EXECUTION_WORKFLOW_NAME, AGENT_EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::agents::domain::{
    AgentEventContent, AgentExecutionEventDraft, AgentExecutionEventKind, AgentExecutionStatus,
};
use crate::modules::agents::{BuiltInAgentExecutionProviderRegistry, InMemoryAgentRepository};
use crate::modules::artifacts::application::project_hosted_build_outcome;
use crate::modules::artifacts::domain::test_support::succeeded_hosted_build;
use crate::modules::artifacts::{HostedArtifactQueryService, InMemoryBuildRunRepository};
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseVersion, AssetReleaseWrite, AssetWrite,
    CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository, TransitionAssetReleaseWrite,
    TransitionAssetWrite,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::InMemoryIdentityRepository;
use crate::modules::operations::{IOperationRepository, InMemoryOperationRepository};
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentApprovalCheckpointId, AgentExecutionId, ApiTokenId, AssetId,
    AssetReleaseId, EnvironmentId, GitCommitSha, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PlanRevisionId, PrincipalId, ProjectId, RepositoryError, ResourceName,
    Sha256Digest, WorkflowRunId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_cloud_contracts::{
    REFERENCE_ECHO_AGENT_PROVIDER_KIND, REFERENCE_ECHO_AGENT_PROVIDER_PROTOCOL_V1,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn approval_checkpoint_list_rejects_limits_outside_the_api_contract() {
    let handler =
        ListAgentApprovalCheckpointsHandler::new(Arc::new(InMemoryAgentRepository::new()));

    for limit in [0, 1_001] {
        let result = handler
            .execute(
                ListAgentApprovalCheckpoints {
                    organization_id: OrganizationId::new(),
                    execution_id: AgentExecutionId::new(),
                    resource_access: ResourceAccessEvaluator::organization_wide(),
                    status: None,
                    limit,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("list Agent approval checkpoints handler");
        assert!(matches!(
            result,
            Err(ApplicationError::Invalid(message))
                if message == "Agent approval checkpoint limit must be between 1 and 1000"
        ));
    }
}

#[tokio::test]
async fn approval_checkpoint_decision_rejects_an_invalid_reason_before_authorization() {
    let handler = DecideAgentApprovalCheckpointHandler::new(
        Arc::new(InMemoryAgentRepository::new()),
        Arc::new(InMemoryIdentityRepository::new()),
    );
    let result = handler
        .execute(
            DecideAgentApprovalCheckpoint {
                organization_id: OrganizationId::new(),
                execution_id: AgentExecutionId::new(),
                checkpoint_id: AgentApprovalCheckpointId::new(),
                expected_version: 1,
                outcome: a3s_cloud_contracts::AgentProviderApprovalOutcomeV1::Denied,
                reason: Some("\u{754c}".repeat(342)),
                resource_access: ResourceAccessEvaluator::organization_wide(),
                actor_principal_id: PrincipalId::new(),
                credential_id: ApiTokenId::new(),
                actor_is_platform_admin: false,
                idempotency_key: "agent-approval:invalid-reason".into(),
                request_id: Uuid::now_v7(),
                requested_at: canonical_timestamp(Utc::now()),
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("decide Agent approval checkpoint handler");
    assert_eq!(
        result,
        Err(ApplicationError::Invalid(
            "Agent approval decision reason is invalid".into()
        ))
    );
}

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
    let artifacts = Arc::new(HostedArtifactQueryService::new(builds));
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

    let start_handler = StartAgentExecutionHandler::new(
        agents.clone(),
        assets,
        artifacts,
        Arc::new(BuiltInAgentExecutionProviderRegistry::new().expect("provider registry")),
    );
    let start = StartAgentExecution {
        organization_id,
        conversation_id: created.conversation.id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        agent_asset_id: asset.id,
        agent_asset_release_id: release.id,
        provider_kind: REFERENCE_ECHO_AGENT_PROVIDER_KIND.into(),
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
    assert_eq!(
        started.execution.provider.kind(),
        REFERENCE_ECHO_AGENT_PROVIDER_KIND
    );
    assert_eq!(
        started.execution.provider.native_protocol(),
        REFERENCE_ECHO_AGENT_PROVIDER_PROTOCOL_V1
    );
    let replayed_execution = start_handler
        .execute(start.clone(), context())
        .await
        .expect("start replay handler")
        .expect("replay execution");
    assert!(replayed_execution.replayed);
    assert_eq!(replayed_execution.execution.id, started.execution.id);
    let mut unknown_provider = start;
    unknown_provider.provider_kind = "unknown.provider".into();
    unknown_provider.idempotency_key = "agent-execution:start:unknown".into();
    assert!(matches!(
        start_handler.execute(unknown_provider, context()).await,
        Ok(Err(ApplicationError::Invalid(message)))
            if message.contains("is not supported")
    ));

    let cancel_handler = CancelAgentExecutionHandler::new(agents.clone());
    let cancel = CancelAgentExecution {
        organization_id,
        execution_id: started.execution.id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "agent-execution:cancel".into(),
        request_id: Uuid::now_v7(),
        requested_at: requested_at + Duration::milliseconds(1),
    };
    let cancelled = cancel_handler
        .execute(cancel.clone(), context())
        .await
        .expect("cancel handler")
        .expect("cancel execution");
    assert_eq!(cancelled.execution.status, AgentExecutionStatus::Cancelling);
    assert_eq!(
        cancelled.execution.cancellation_requested_at,
        Some(cancel.requested_at)
    );
    let replayed_cancellation = cancel_handler
        .execute(cancel, context())
        .await
        .expect("cancel replay handler")
        .expect("replay cancellation");
    assert!(replayed_cancellation.replayed);
    assert_eq!(replayed_cancellation.execution, cancelled.execution);
    let outbox = agents.outbox_events().await;
    assert_eq!(outbox.len(), 3);
    assert_eq!(
        outbox[2].event_key,
        "agent.execution.cancellation-requested"
    );

    let operations = Arc::new(InMemoryOperationRepository::new());
    let reconciler = AgentExecutionReconciler::new(agents.clone(), operations.clone());
    let first_reconcile = reconciler.run_once(100).await.expect("reconcile Agent run");
    assert_eq!(first_reconcile.started, 1);
    assert_eq!(first_reconcile.replayed, 0);
    assert!(first_reconcile.failures.is_empty());
    let operation = operations
        .find_request(started.execution.operation_id)
        .await
        .expect("find Agent operation")
        .expect("Agent operation");
    assert_eq!(operation.workflow.name(), AGENT_EXECUTION_WORKFLOW_NAME);
    assert_eq!(
        operation.workflow.version(),
        AGENT_EXECUTION_WORKFLOW_VERSION
    );
    assert_eq!(operation.subject.kind(), "agent_execution");
    assert_eq!(operation.subject.id(), started.execution.id.as_uuid());
    assert_eq!(
        operation.input,
        serde_json::json!({
            "organizationId": organization_id,
            "executionId": started.execution.id,
        })
    );
    let replay_reconcile = reconciler
        .run_once(100)
        .await
        .expect("reconcile Agent replay");
    assert_eq!(replay_reconcile.started, 0);
    assert_eq!(replay_reconcile.replayed, 1);

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
                resource_access: ResourceAccessEvaluator::organization_wide(),
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

#[tokio::test]
async fn workflow_agent_port_pins_release_replays_output_and_cancellation() {
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
        ResourceName::parse("workflow-agent").expect("Asset name"),
        AssetKind::Agent,
        drafted_at,
    )
    .expect("Agent Asset");
    let (release, build) = published_release(&asset, drafted_at);
    let release_digest = release
        .artifact
        .as_ref()
        .expect("published artifact")
        .digest()
        .clone();
    let environments = Arc::new(TestEnvironmentRepository { environment });
    let assets = Arc::new(TestAssetRepository {
        asset: asset.clone(),
        release: release.clone(),
    });
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    builds.seed_build(build).await;
    let artifacts = Arc::new(HostedArtifactQueryService::new(builds));
    let agents = Arc::new(InMemoryAgentRepository::new());
    let service =
        WorkflowAgentApplicationService::new(environments, agents.clone(), assets, artifacts);
    let request = WorkflowAgentRequest {
        organization_id,
        project_id,
        environment_id,
        workflow_run_id: WorkflowRunId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest: Sha256Digest::parse(format!("sha256:{}", "c".repeat(64)))
            .expect("plan digest"),
        step_id: "agent".into(),
        step_attempt: 1,
        agent_asset_id: asset.id,
        agent_asset_release_id: release.id,
        agent_release_digest: release_digest,
        capability: "agent.execute".into(),
        input: serde_json::json!({"message": "hello"}),
        requested_at,
    };

    let started = service
        .start_or_adopt(&request)
        .await
        .expect("start Workflow Agent execution");
    let replayed = service
        .start_or_adopt(&request)
        .await
        .expect("replay Workflow Agent execution");
    assert_eq!(replayed, started);

    let context = || CqrsContext::new(ModuleRef::new());
    let append_handler = AppendAgentExecutionEventsHandler::new(agents.clone());
    let appended = append_handler
        .execute(
            AppendAgentExecutionEvents {
                organization_id,
                conversation_id: started.conversation_id,
                execution_id: started.id,
                events: vec![
                    event(
                        AgentExecutionEventKind::ModelOutput,
                        serde_json::json!({"text": "hello "}),
                        requested_at + Duration::seconds(1),
                    ),
                    event(
                        AgentExecutionEventKind::ModelOutput,
                        serde_json::json!({"text": "world"}),
                        requested_at + Duration::seconds(1),
                    ),
                    event(
                        AgentExecutionEventKind::ExecutionCompleted,
                        serde_json::json!({}),
                        requested_at + Duration::seconds(1),
                    ),
                ],
                idempotency_key: "workflow-agent:events:complete".into(),
            },
            context(),
        )
        .await
        .expect("append Workflow Agent events")
        .expect("append Workflow Agent events result");
    let observation = service
        .terminal_observation(&request, &appended.execution)
        .await
        .expect("observe Workflow Agent terminal output")
        .expect("terminal Workflow Agent observation");
    assert_eq!(observation.execution, appended.execution);
    assert_eq!(observation.output_text, "hello world");
    assert_eq!(observation.terminal_event_sequence, 4);

    let mut drifted = request.clone();
    drifted.agent_release_digest =
        Sha256Digest::parse(format!("sha256:{}", "f".repeat(64))).expect("drift digest");
    assert!(matches!(
        service.start_or_adopt(&drifted).await,
        Err(ApplicationError::Conflict(_))
    ));

    let mut cancellation_request = request.clone();
    cancellation_request.workflow_run_id = WorkflowRunId::new();
    cancellation_request.requested_at = requested_at + Duration::seconds(10);
    let cancellable = service
        .start_or_adopt(&cancellation_request)
        .await
        .expect("start cancellable Workflow Agent execution");
    let cancellation_at = cancellation_request.requested_at + Duration::seconds(1);
    let cancelling = service
        .request_cancellation(&cancellation_request, cancellation_at)
        .await
        .expect("request Workflow Agent cancellation")
        .expect("cancelling Workflow Agent execution");
    assert_eq!(cancelling.id, cancellable.id);
    assert_eq!(cancelling.status, AgentExecutionStatus::Cancelling);
    let replayed_cancellation = service
        .request_cancellation(&cancellation_request, cancellation_at)
        .await
        .expect("replay Workflow Agent cancellation")
        .expect("replayed cancelling execution");
    assert_eq!(replayed_cancellation, cancelling);
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
    let outcome = project_hosted_build_outcome(&build)
        .expect("project hosted outcome")
        .expect("successful hosted outcome");
    release
        .publish_from_hosted_build(asset, &outcome)
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
