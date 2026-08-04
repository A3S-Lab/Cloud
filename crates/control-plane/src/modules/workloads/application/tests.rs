use super::{
    CreateAgentWorkloadDeployment, CreateAgentWorkloadDeploymentHandler, SourceWorkloadTemplate,
    UpdateAgentWorkloadDeployment, UpdateAgentWorkloadDeploymentHandler, UpdateWorkloadDeployment,
    UpdateWorkloadDeploymentHandler,
};
use crate::modules::artifacts::domain::test_support::succeeded_hosted_build;
use crate::modules::artifacts::{BuildRun, InMemoryBuildRunRepository};
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseVersion, AssetReleaseWrite, AssetWrite,
    CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository, TransitionAssetReleaseWrite,
    TransitionAssetWrite,
};
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::secrets::InMemorySecretRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, EnvironmentId, GitCommitSha, IdempotencyRequest,
    IdempotentWrite, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError,
    ResourceName, Sha256Digest,
};
use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, ServicePort, ServiceProcess, ServiceResources,
};
use crate::modules::workloads::{IWorkloadRepository, InMemoryWorkloadRepository};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[tokio::test]
async fn agent_release_deploy_update_and_replay_reuse_the_workload_lifecycle() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let drafted_at = canonical_timestamp(Utc::now() - Duration::minutes(1));
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("research-agent").expect("Asset name"),
        AssetKind::Agent,
        drafted_at,
    )
    .expect("Agent Asset");
    let (release_one, build_one) = published_release(&asset, "1.0.0", 'a', drafted_at);
    let (release_two, build_two) = published_release(&asset, "2.0.0", 'c', drafted_at);
    let requested_at = release_one.updated_at.max(release_two.updated_at) + Duration::seconds(1);
    let assets = Arc::new(AgentAssetStore::new(
        asset.clone(),
        [release_one.clone(), release_two.clone()],
    ));
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    builds.seed_build(build_one.clone()).await;
    builds.seed_build(build_two.clone()).await;
    let environments = Arc::new(TestEnvironmentRepository {
        environment: Environment::create(
            organization_id,
            project_id,
            environment_id,
            EnvironmentName::parse("Production").expect("Environment name"),
            drafted_at,
        ),
    });
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let secrets = Arc::new(InMemorySecretRepository::new());
    let create_handler = CreateAgentWorkloadDeploymentHandler::new(
        environments,
        assets.clone(),
        builds.clone(),
        workloads.clone(),
        secrets.clone(),
    );
    let create = CreateAgentWorkloadDeployment {
        organization_id,
        project_id,
        environment_id,
        asset_id: asset.id,
        asset_release_id: release_one.id,
        name: "research-runtime".into(),
        template: source_template(),
        idempotency_key: "agent-release:create".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    };

    let created = create_handler
        .execute(create.clone(), context())
        .await
        .expect("create handler")
        .expect("create Agent Workload");
    let create_binding = created
        .bundle
        .revision
        .agent_binding()
        .expect("Agent release binding");
    assert_eq!(create_binding.asset_id(), asset.id);
    assert_eq!(create_binding.asset_release_id(), release_one.id);
    assert_eq!(create_binding.build_run_id(), build_one.id);
    assert_eq!(
        created
            .bundle
            .revision
            .resolved_template()
            .expect("resolved template")
            .artifact
            .uri,
        build_one
            .published_artifact
            .as_ref()
            .expect("published artifact")
            .uri
    );
    let create_replay = create_handler
        .execute(create, context())
        .await
        .expect("create replay handler")
        .expect("create replay");
    assert!(create_replay.bundle.replayed);
    assert_eq!(
        create_replay.bundle.deployment.id,
        created.bundle.deployment.id
    );

    activate(
        workloads.as_ref(),
        organization_id,
        created.bundle.deployment.id,
        requested_at + Duration::seconds(1),
    )
    .await;
    let update_handler = UpdateAgentWorkloadDeploymentHandler::new(
        assets.clone(),
        builds,
        workloads.clone(),
        secrets.clone(),
    );
    let update = UpdateAgentWorkloadDeployment {
        organization_id,
        workload_id: created.bundle.workload.id,
        asset_id: asset.id,
        asset_release_id: release_two.id,
        expected_name: Some("research-runtime".into()),
        template: source_template(),
        idempotency_key: "agent-release:update".into(),
        request_id: Uuid::now_v7(),
        requested_at: requested_at + Duration::seconds(2),
    };
    let updated = update_handler
        .execute(update.clone(), context())
        .await
        .expect("update handler")
        .expect("update Agent Workload");
    assert_eq!(updated.bundle.revision.generation, 2);
    assert_eq!(
        updated
            .bundle
            .revision
            .agent_binding()
            .expect("updated Agent binding")
            .asset_release_id(),
        release_two.id
    );

    assets
        .yank(release_two.id, update.requested_at + Duration::seconds(1))
        .await;
    let update_replay = update_handler
        .execute(update.clone(), context())
        .await
        .expect("update replay handler")
        .expect("update replay after yank");
    assert!(update_replay.bundle.replayed);
    assert_eq!(
        update_replay.bundle.deployment.id,
        updated.bundle.deployment.id
    );

    let mut fresh_yanked_update = update;
    fresh_yanked_update.idempotency_key = "agent-release:update-yanked".into();
    fresh_yanked_update.request_id = Uuid::now_v7();
    let rejected = update_handler
        .execute(fresh_yanked_update, context())
        .await
        .expect("yanked update handler");
    assert!(matches!(rejected, Err(ApplicationError::Conflict(_))));

    let ordinary_update = UpdateWorkloadDeploymentHandler::new(workloads, secrets)
        .execute(
            UpdateWorkloadDeployment {
                organization_id,
                workload_id: created.bundle.workload.id,
                expected_name: None,
                template: created
                    .bundle
                    .revision
                    .resolved_template()
                    .expect("resolved template")
                    .clone(),
                idempotency_key: "agent-release:ordinary-update".into(),
                request_id: Uuid::now_v7(),
                requested_at: requested_at + Duration::seconds(4),
            },
            context(),
        )
        .await
        .expect("ordinary update handler");
    assert!(matches!(
        ordinary_update,
        Err(ApplicationError::Conflict(message))
            if message.contains("release lifecycle")
    ));
}

fn published_release(
    asset: &Asset,
    version: &str,
    character: char,
    drafted_at: chrono::DateTime<Utc>,
) -> (AssetRelease, BuildRun) {
    let mut release = AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse(version).expect("release version"),
        GitCommitSha::parse(character.to_string().repeat(40)).expect("commit SHA"),
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
            .expect("manifest digest"),
        drafted_at,
    )
    .expect("draft release");
    let build = succeeded_hosted_build(asset.organization_id, asset.id, release.id, drafted_at);
    release
        .publish_from_build(asset, &build)
        .expect("publish release");
    (release, build)
}

fn source_template() -> SourceWorkloadTemplate {
    SourceWorkloadTemplate {
        process: ServiceProcess {
            command: vec!["/app/agent".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 33_554_432,
            pids: 32,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: Some(HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 1_000,
        }),
    }
}

async fn activate(
    workloads: &InMemoryWorkloadRepository,
    organization_id: OrganizationId,
    deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
    mut at: chrono::DateTime<Utc>,
) {
    let deployment = workloads
        .find_deployment(organization_id, deployment_id)
        .await
        .expect("deployment");
    at = at.max(deployment.updated_at);
    let deployment = workloads
        .mark_resolving(deployment.id, deployment.aggregate_version, at)
        .await
        .expect("mark resolving");
    let deployment = workloads
        .assign_node(
            deployment.id,
            deployment.aggregate_version,
            NodeId::new(),
            at + Duration::milliseconds(1),
        )
        .await
        .expect("assign node");
    let deployment = workloads
        .mark_dispatched(
            deployment.id,
            deployment.aggregate_version,
            NodeCommandId::new(),
            at + Duration::milliseconds(2),
        )
        .await
        .expect("mark dispatched");
    let deployment = workloads
        .mark_verifying(
            deployment.id,
            deployment.aggregate_version,
            at + Duration::milliseconds(3),
        )
        .await
        .expect("mark verifying");
    workloads
        .activate(
            deployment.id,
            deployment.aggregate_version,
            false,
            at + Duration::milliseconds(4),
        )
        .await
        .expect("activate Workload");
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
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

struct AgentAssetStore {
    asset: Asset,
    releases: RwLock<HashMap<AssetReleaseId, AssetRelease>>,
}

impl AgentAssetStore {
    fn new(asset: Asset, releases: impl IntoIterator<Item = AssetRelease>) -> Self {
        Self {
            asset,
            releases: RwLock::new(
                releases
                    .into_iter()
                    .map(|release| (release.id, release))
                    .collect(),
            ),
        }
    }

    async fn yank(&self, release_id: AssetReleaseId, at: chrono::DateTime<Utc>) {
        self.releases
            .write()
            .await
            .get_mut(&release_id)
            .expect("release")
            .yank(at)
            .expect("yank release");
    }
}

#[async_trait]
impl IAssetRepository for AgentAssetStore {
    async fn create_asset(&self, _bundle: CreateAssetWrite) -> Result<AssetWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused Asset write".into()))
    }

    async fn transition_asset(
        &self,
        _bundle: TransitionAssetWrite,
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
        _bundle: CreateAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused release write".into()))
    }

    async fn transition_release(
        &self,
        _bundle: TransitionAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused release transition".into()))
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<AssetRelease>, RepositoryError> {
        if self.asset.organization_id != organization_id || self.asset.id != asset_id {
            return Ok(None);
        }
        Ok(self.releases.read().await.get(&asset_release_id).cloned())
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Vec<AssetRelease>, RepositoryError> {
        if self.asset.organization_id != organization_id || self.asset.id != asset_id {
            return Ok(Vec::new());
        }
        Ok(self.releases.read().await.values().cloned().collect())
    }
}
