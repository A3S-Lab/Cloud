//! GA-1 management-plane Postgres gate (no Box / no Gateway / no `/dev/kvm`).
//!
//! Proves Agents-owned CreateAgentConversation → StartAgentExecution →
//! GetAgentExecutionEvents against a published Agent release, and emits
//! `A3S_CLOUD_GA1_MANAGEMENT_PLANE_PROVEN`. This is not LIVE CERTIFIED and must
//! not invent GA-1 Verified.

use crate::migrate_and_connect_for_test;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_control_plane::conformance::agent_organization_access_for_conformance;
use a3s_cloud_control_plane::modules::agents::{
    AssetsAgentReleaseAdmissionAdapter, BuiltInAgentExecutionProviderRegistry,
    CreateAgentConversation, CreateAgentConversationHandler, GetAgentExecutionEvents,
    GetAgentExecutionEventsHandler, PostgresAgentRepository, ProjectsAgentsEnvironmentAccessAdapter,
    StartAgentExecution, StartAgentExecutionHandler, AgentExecutionEventKind,
    NATIVE_CODE_AGENT_PROVIDER_KIND,
};
use a3s_cloud_control_plane::modules::artifacts::{
    HostedArtifactQueryService, PostgresBuildRunRepository,
};
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetCreated, AssetKind, AssetRelease, AssetReleaseDrafted, AssetReleaseVersion,
    CreateAssetReleaseWrite, CreateAssetWrite, HostedAssetBuildRequested, IAssetRepository,
    PostgresAssetRepository,
};
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, EnvironmentId, GitCommitSha, IdempotencyRequest, OrganizationId,
    ProjectId, ResourceName, Sha256Digest,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{DateTime, Duration, Timelike, Utc};
use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug)]
pub struct ManagementPlaneProof {
    pub event_count: usize,
    pub head_sequence: u64,
}

pub async fn exercise_ga1_management_plane_postgres(postgres_url: String) -> TestResult {
    let executor = migrate_and_connect_for_test(&postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let created_at = canonical_timestamp(Utc::now() - Duration::seconds(10));
    insert_scope(
        &database,
        organization_id,
        project_id,
        environment_id,
        created_at,
    )
    .await?;

    let assets = Arc::new(PostgresAssetRepository::new(executor.clone()));
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("GA-1 Management Plane Agent")?,
        AssetKind::Agent,
        created_at,
    )?;
    assets
        .create_asset(CreateAssetWrite {
            asset: asset.clone(),
            event: AssetCreated::envelope(&asset, Uuid::now_v7())?,
            idempotency: idempotency(
                &format!("test.ga1-mp.organizations/{organization_id}/assets"),
                "create-agent",
                b"create-agent",
            )?,
        })
        .await?;
    let release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0")?,
        GitCommitSha::parse("a".repeat(40))?,
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))?,
        created_at + Duration::milliseconds(1),
    )?;
    assets
        .create_release(CreateAssetReleaseWrite {
            release: release.clone(),
            event: AssetReleaseDrafted::envelope(&release, release.id.as_uuid())?,
            hosted_build_requested_event: Some(HostedAssetBuildRequested::envelope(
                &asset,
                &release,
                release.id.as_uuid(),
            )?),
            idempotency: idempotency(
                &format!("test.ga1-mp.organizations/{organization_id}/releases"),
                "draft-agent-1.0.0",
                b"draft-agent-1.0.0",
            )?,
        })
        .await?;
    let published =
        crate::build_runs_support::publish_hosted_release(&executor, &asset, &release).await?;

    let proof = prove_management_plane_conversation_and_events(
        organization_id,
        project_id,
        environment_id,
        asset.id,
        published.id,
        executor,
    )
    .await?;
    if proof.event_count == 0 || proof.head_sequence == 0 {
        return Err("GA-1 management plane proof returned empty events".into());
    }
    Ok(())
}

pub async fn prove_management_plane_conversation_and_events(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    executor: PostgresExecutor,
) -> TestResult<ManagementPlaneProof> {
    let projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let agents = Arc::new(PostgresAgentRepository::new(executor.clone()));
    let assets = Arc::new(PostgresAssetRepository::new(executor.clone()));
    let artifacts = Arc::new(HostedArtifactQueryService::new(Arc::new(
        PostgresBuildRunRepository::new(executor),
    )));
    let access = agent_organization_access_for_conformance();
    let requested_at = Utc::now();

    let conversation = CreateAgentConversationHandler::new(
        Arc::new(ProjectsAgentsEnvironmentAccessAdapter::new(projects)),
        agents.clone(),
    )
    .execute(
        CreateAgentConversation {
            organization_id,
            project_id,
            environment_id,
            access: access.clone(),
            idempotency_key: "ga1-mp-conversation".into(),
            request_id: Uuid::now_v7(),
            requested_at,
        },
        CqrsContext::new(ModuleRef::new()),
    )
    .await?
    .map_err(|error| format!("GA-1 could not create Agent conversation: {error}"))?;
    if conversation.replayed {
        return Err("GA-1 conversation must be a first write, not a replay".into());
    }

    let execution = StartAgentExecutionHandler::new(
        agents.clone(),
        Arc::new(AssetsAgentReleaseAdmissionAdapter::new(assets, artifacts)),
        Arc::new(BuiltInAgentExecutionProviderRegistry::new().map_err(|error| error.to_string())?),
    )
    .execute(
        StartAgentExecution {
            organization_id,
            conversation_id: conversation.conversation.id,
            access: access.clone(),
            agent_asset_id: asset_id,
            agent_asset_release_id: asset_release_id,
            provider_kind: NATIVE_CODE_AGENT_PROVIDER_KIND.into(),
            input: serde_json::json!({"prompt": "ga1-management-plane"}),
            idempotency_key: "ga1-mp-execution".into(),
            request_id: Uuid::now_v7(),
            requested_at: requested_at + Duration::milliseconds(1),
        },
        CqrsContext::new(ModuleRef::new()),
    )
    .await?
    .map_err(|error| format!("GA-1 could not start Agent execution: {error}"))?;
    if execution.replayed {
        return Err("GA-1 execution must be a first write, not a replay".into());
    }

    let events = GetAgentExecutionEventsHandler::new(agents)
        .execute(
            GetAgentExecutionEvents {
                organization_id,
                conversation_id: conversation.conversation.id,
                access,
                after_sequence: None,
                limit: 50,
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await?
        .map_err(|error| format!("GA-1 could not list Agent events: {error}"))?;
    if events.records.is_empty() || events.head_sequence == 0 {
        return Err(
            "GA-1 management plane required conversations>=1 events>=1 against the published release"
                .into(),
        );
    }
    if !events
        .records
        .iter()
        .any(|event| event.kind == AgentExecutionEventKind::ExecutionRequested)
    {
        return Err(
            "GA-1 management plane required a durable ExecutionRequested event for the started execution"
                .into(),
        );
    }
    println!(
        "A3S_CLOUD_GA1_MANAGEMENT_PLANE_PROVEN conversations=1 executions=1 events={} \
         conversation_id={} execution_id={} head_sequence={}",
        events.records.len(),
        conversation.conversation.id,
        execution.execution.id,
        events.head_sequence
    );
    Ok(ManagementPlaneProof {
        event_count: events.records.len(),
        head_sequence: events.head_sequence,
    })
}

async fn insert_scope(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    created_at: chrono::DateTime<Utc>,
) -> TestResult {
    let scope_suffix = organization_id.as_uuid().simple().to_string();
    let organization_name = format!("GA-1 MP tenant {scope_suffix}");
    let organization_name_key = format!("ga1-mp-tenant-{scope_suffix}");
    let project_name = format!("GA-1 MP project {scope_suffix}");
    let project_name_key = format!("ga1-mp-project-{scope_suffix}");
    let environment_name = format!("GA-1 MP environment {scope_suffix}");
    let environment_name_key = format!("ga1-mp-environment-{scope_suffix}");
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(organization_name)
            .append(", ")
            .bind(organization_name_key)
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", ")
            .bind(project_name)
            .append(", ")
            .bind(project_name_key)
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", ")
            .bind(environment_id.as_uuid())
            .append(", ")
            .bind(environment_name)
            .append(", ")
            .bind(environment_name_key)
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    Ok(())
}

fn idempotency(scope: &str, key: &str, body: &[u8]) -> Result<IdempotencyRequest, String> {
    IdempotencyRequest::new(scope, key, body)
}

fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    value
        - chrono::TimeDelta::nanoseconds(i64::from(value.nanosecond() % 1_000))
}
