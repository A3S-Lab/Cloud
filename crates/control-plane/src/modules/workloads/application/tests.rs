use super::{
    BindSkillWorkloadDeployment, BindSkillWorkloadDeploymentHandler, CreateAgentWorkloadDeployment,
    CreateAgentWorkloadDeploymentHandler, SourceWorkloadTemplate, UnbindSkillWorkloadDeployment,
    UnbindSkillWorkloadDeploymentHandler, UpdateAgentWorkloadDeployment,
    UpdateAgentWorkloadDeploymentHandler, UpdateWorkloadDeployment,
    UpdateWorkloadDeploymentHandler,
};
use crate::modules::artifacts::application::project_hosted_build_outcome;
use crate::modules::artifacts::domain::test_support::succeeded_hosted_agent_build;
use crate::modules::artifacts::{BuildRun, HostedArtifactQueryService, InMemoryBuildRunRepository};
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseArtifact, AssetReleaseVersion, AssetReleaseWrite,
    AssetWrite, CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository,
    TransitionAssetReleaseWrite, TransitionAssetWrite,
};
use crate::modules::fleet::domain::entities::NodePool;
use crate::modules::fleet::domain::repositories::{INodePoolRepository, NodePoolWrite};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::secrets::InMemorySecretRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, EnvironmentId, GitCommitSha, IdempotencyRequest,
    IdempotentWrite, NodeCommandId, NodeId, NodePoolId, OrganizationId, ProjectId, RepositoryError,
    ResourceName, Sha256Digest,
};
use crate::modules::workloads::domain::entities::{ServicePort, ServiceProcess, ServiceResources};
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
    let node_id = NodeId::new();
    let drafted_at = canonical_timestamp(Utc::now() - Duration::minutes(1));
    let node_pool = NodePool::create(
        NodePoolId::new(),
        organization_id,
        ResourceName::parse("agent workers").expect("node pool name"),
        vec![node_id],
        drafted_at,
    )
    .expect("node pool");
    let node_pool_id = node_pool.id;
    let node_pools = Arc::new(TestNodePoolRepository { pool: node_pool });
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("research-agent").expect("Asset name"),
        AssetKind::Agent,
        drafted_at,
    )
    .expect("Agent Asset");
    let (release_one, build_one) = published_release(&asset, "1.0.0", drafted_at);
    let (release_two, build_two) = published_release(&asset, "2.0.0", drafted_at);
    let requested_at = release_one.updated_at.max(release_two.updated_at) + Duration::seconds(1);
    let assets = Arc::new(AgentAssetStore::new(
        asset.clone(),
        [release_one.clone(), release_two.clone()],
    ));
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    builds.seed_build(build_one.clone()).await;
    builds.seed_build(build_two).await;
    let artifacts = Arc::new(HostedArtifactQueryService::new(builds));
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
        artifacts.clone(),
        workloads.clone(),
        secrets.clone(),
        node_pools,
    );
    let create = CreateAgentWorkloadDeployment {
        organization_id,
        project_id,
        environment_id,
        asset_id: asset.id,
        asset_release_id: release_one.id,
        name: "research-runtime".into(),
        node_pool_id: Some(node_pool_id),
        template: source_template(),
        idempotency_key: "agent-release:create".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    };

    let mut process_override = create.clone();
    process_override.template.process.command = vec!["/caller-selected-entrypoint".into()];
    process_override.idempotency_key = "agent-release:process-override".into();
    process_override.request_id = Uuid::now_v7();
    let rejected = create_handler
        .execute(process_override, context())
        .await
        .expect("process override handler");
    assert!(matches!(
        rejected,
        Err(ApplicationError::Invalid(message))
            if message.contains("derived from its release manifest")
    ));

    let mut port_override = create.clone();
    port_override.template.ports = vec![ServicePort {
        name: "caller".into(),
        container_port: 9_999,
    }];
    port_override.idempotency_key = "agent-release:port-override".into();
    port_override.request_id = Uuid::now_v7();
    let rejected = create_handler
        .execute(port_override, context())
        .await
        .expect("port override handler");
    assert!(matches!(
        rejected,
        Err(ApplicationError::Invalid(message))
            if message.contains("derived from its release manifest")
    ));

    let mut unbounded_storage = create.clone();
    unbounded_storage.template.resources.ephemeral_storage_bytes = None;
    unbounded_storage.idempotency_key = "agent-release:unbounded-storage".into();
    unbounded_storage.request_id = Uuid::now_v7();
    let rejected = create_handler
        .execute(unbounded_storage, context())
        .await
        .expect("unbounded storage handler");
    assert!(matches!(
        rejected,
        Err(ApplicationError::Invalid(message)) if message.contains("ephemeral storage")
    ));

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
        workloads
            .find_workload_control(organization_id, created.bundle.workload.id)
            .await
            .expect("workload control")
            .spec
            .placement_policy
            .node_pool_id(),
        Some(node_pool_id)
    );
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
    let mut missing_pool_create = create.clone();
    missing_pool_create.name = "research-runtime-missing-pool".into();
    missing_pool_create.node_pool_id = Some(NodePoolId::new());
    missing_pool_create.idempotency_key = "agent-release:create-missing-pool".into();
    let missing_pool = create_handler
        .execute(missing_pool_create, context())
        .await
        .expect("missing node pool handler");
    assert!(matches!(
        missing_pool,
        Err(ApplicationError::NotFound(message)) if message.contains("node pool")
    ));
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
        node_id,
        requested_at + Duration::seconds(1),
    )
    .await;
    let update_handler = UpdateAgentWorkloadDeploymentHandler::new(
        assets.clone(),
        artifacts,
        workloads.clone(),
        secrets.clone(),
    );
    let update = UpdateAgentWorkloadDeployment {
        organization_id,
        workload_id: created.bundle.workload.id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        asset_id: asset.id,
        asset_release_id: release_two.id,
        expected_name: Some("research-runtime".into()),
        expected_node_pool_id: Some(Some(node_pool_id)),
        template: source_template(),
        idempotency_key: "agent-release:update".into(),
        request_id: Uuid::now_v7(),
        requested_at: requested_at + Duration::seconds(2),
    };
    let mut mismatched_placement = update.clone();
    mismatched_placement.expected_node_pool_id = Some(Some(NodePoolId::new()));
    mismatched_placement.idempotency_key = "agent-release:update-wrong-pool".into();
    let mismatch = update_handler
        .execute(mismatched_placement, context())
        .await
        .expect("mismatched placement handler");
    assert!(matches!(
        mismatch,
        Err(ApplicationError::Conflict(message)) if message.contains("immutable target node pool")
    ));
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
                resource_access: ResourceAccessEvaluator::organization_wide(),
                expected_name: None,
                expected_node_pool_id: None,
                template: created.bundle.revision.request.clone(),
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

#[tokio::test]
async fn skill_bind_rebind_agent_update_and_unbind_preserve_exact_revision_history() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let node_id = NodeId::new();
    let drafted_at = canonical_timestamp(Utc::now() - Duration::minutes(1));
    let node_pool = NodePool::create(
        NodePoolId::new(),
        organization_id,
        ResourceName::parse("skill host workers").expect("node pool name"),
        vec![node_id],
        drafted_at,
    )
    .expect("node pool");
    let node_pool_id = node_pool.id;
    let agent = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("skill-host-agent").expect("Agent Asset name"),
        AssetKind::Agent,
        drafted_at,
    )
    .expect("Agent Asset");
    let (agent_release, agent_build) = published_release(&agent, "1.0.0", drafted_at);
    let skill = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("research-tools").expect("Skill Asset name"),
        AssetKind::Skill,
        drafted_at,
    )
    .expect("Skill Asset");
    let skill_release_one = published_skill_release(&skill, "1.0.0", 'b', drafted_at);
    let skill_release_two = published_skill_release(&skill, "2.0.0", 'c', drafted_at);
    let requested_at = [
        agent_release.updated_at,
        skill_release_one.updated_at,
        skill_release_two.updated_at,
    ]
    .into_iter()
    .max()
    .expect("latest release timestamp")
        + Duration::seconds(1);
    let agent_assets = Arc::new(AgentAssetStore::new(agent.clone(), [agent_release.clone()]));
    let skill_assets = Arc::new(AgentAssetStore::new(
        skill.clone(),
        [skill_release_one.clone(), skill_release_two.clone()],
    ));
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    builds.seed_build(agent_build).await;
    let artifacts = Arc::new(HostedArtifactQueryService::new(builds));
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
    let created = CreateAgentWorkloadDeploymentHandler::new(
        environments,
        agent_assets.clone(),
        artifacts.clone(),
        workloads.clone(),
        secrets.clone(),
        Arc::new(TestNodePoolRepository { pool: node_pool }),
    )
    .execute(
        CreateAgentWorkloadDeployment {
            organization_id,
            project_id,
            environment_id,
            asset_id: agent.id,
            asset_release_id: agent_release.id,
            name: "skill-host-runtime".into(),
            node_pool_id: Some(node_pool_id),
            template: source_template(),
            idempotency_key: "skill-binding:create-agent".into(),
            request_id: Uuid::now_v7(),
            requested_at,
        },
        context(),
    )
    .await
    .expect("create handler")
    .expect("create Agent Workload");
    activate(
        workloads.as_ref(),
        organization_id,
        created.bundle.deployment.id,
        node_id,
        requested_at + Duration::seconds(1),
    )
    .await;

    let bind_handler = BindSkillWorkloadDeploymentHandler::new(
        skill_assets.clone(),
        workloads.clone(),
        secrets.clone(),
    );
    let bind_one = BindSkillWorkloadDeployment {
        organization_id,
        workload_id: created.bundle.workload.id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        skill_asset_id: skill.id,
        skill_asset_release_id: skill_release_one.id,
        idempotency_key: "skill-binding:bind-one".into(),
        request_id: Uuid::now_v7(),
        requested_at: requested_at + Duration::seconds(2),
    };
    let bound = bind_handler
        .execute(bind_one.clone(), context())
        .await
        .expect("bind handler")
        .expect("bind Skill");
    assert_eq!(bound.bundle.revision.generation, 2);
    assert_eq!(
        bound
            .bundle
            .revision
            .skill_binding(skill.id)
            .expect("Skill binding")
            .asset_release_id(),
        skill_release_one.id
    );
    skill_assets
        .yank(skill_release_one.id, requested_at + Duration::seconds(3))
        .await;
    let replayed = bind_handler
        .execute(bind_one.clone(), context())
        .await
        .expect("bind replay handler")
        .expect("bind replay after yank");
    assert!(replayed.bundle.replayed);
    assert_eq!(replayed.bundle.deployment.id, bound.bundle.deployment.id);
    let mut rejected_bind = bind_one;
    rejected_bind.idempotency_key = "skill-binding:yanked-fresh".into();
    rejected_bind.request_id = Uuid::now_v7();
    let rejected = bind_handler
        .execute(rejected_bind, context())
        .await
        .expect("fresh yanked bind handler");
    assert!(matches!(rejected, Err(ApplicationError::Conflict(_))));

    activate(
        workloads.as_ref(),
        organization_id,
        bound.bundle.deployment.id,
        node_id,
        requested_at + Duration::seconds(4),
    )
    .await;
    let rebound = bind_handler
        .execute(
            BindSkillWorkloadDeployment {
                organization_id,
                workload_id: created.bundle.workload.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                skill_asset_id: skill.id,
                skill_asset_release_id: skill_release_two.id,
                idempotency_key: "skill-binding:bind-two".into(),
                request_id: Uuid::now_v7(),
                requested_at: requested_at + Duration::seconds(5),
            },
            context(),
        )
        .await
        .expect("rebind handler")
        .expect("rebind Skill");
    assert_eq!(rebound.bundle.revision.generation, 3);
    assert_eq!(
        rebound
            .bundle
            .revision
            .skill_binding(skill.id)
            .expect("rebound Skill")
            .asset_release_id(),
        skill_release_two.id
    );
    assert_eq!(
        workloads
            .find_revision(organization_id, bound.bundle.revision.id)
            .await
            .expect("original bound revision")
            .skill_binding(skill.id)
            .expect("original Skill binding")
            .asset_release_id(),
        skill_release_one.id
    );

    activate(
        workloads.as_ref(),
        organization_id,
        rebound.bundle.deployment.id,
        node_id,
        requested_at + Duration::seconds(6),
    )
    .await;
    let updated_agent = UpdateAgentWorkloadDeploymentHandler::new(
        agent_assets,
        artifacts,
        workloads.clone(),
        secrets.clone(),
    )
    .execute(
        UpdateAgentWorkloadDeployment {
            organization_id,
            workload_id: created.bundle.workload.id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            asset_id: agent.id,
            asset_release_id: agent_release.id,
            expected_name: None,
            expected_node_pool_id: None,
            template: source_template(),
            idempotency_key: "skill-binding:update-agent".into(),
            request_id: Uuid::now_v7(),
            requested_at: requested_at + Duration::seconds(7),
        },
        context(),
    )
    .await
    .expect("Agent update handler")
    .expect("Agent update");
    assert_eq!(updated_agent.bundle.revision.generation, 4);
    assert_eq!(
        updated_agent
            .bundle
            .revision
            .skill_binding(skill.id)
            .expect("preserved Skill binding")
            .asset_release_id(),
        skill_release_two.id
    );

    activate(
        workloads.as_ref(),
        organization_id,
        updated_agent.bundle.deployment.id,
        node_id,
        requested_at + Duration::seconds(8),
    )
    .await;
    let unbind_handler =
        UnbindSkillWorkloadDeploymentHandler::new(workloads.clone(), secrets.clone());
    let unbind = UnbindSkillWorkloadDeployment {
        organization_id,
        workload_id: created.bundle.workload.id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        skill_asset_id: skill.id,
        idempotency_key: "skill-binding:unbind".into(),
        request_id: Uuid::now_v7(),
        requested_at: requested_at + Duration::seconds(9),
    };
    let unbound = unbind_handler
        .execute(unbind.clone(), context())
        .await
        .expect("unbind handler")
        .expect("unbind Skill");
    assert_eq!(unbound.bundle.revision.generation, 5);
    assert!(unbound.bundle.revision.skill_bindings().is_empty());
    let unbind_replay = unbind_handler
        .execute(unbind, context())
        .await
        .expect("unbind replay handler")
        .expect("unbind replay");
    assert!(unbind_replay.bundle.replayed);
    assert_eq!(
        unbind_replay.bundle.deployment.id,
        unbound.bundle.deployment.id
    );
    assert_eq!(
        workloads
            .find_workload_control(organization_id, created.bundle.workload.id)
            .await
            .expect("preserved Workload control")
            .spec
            .placement_policy
            .node_pool_id(),
        Some(node_pool_id)
    );
}

fn published_release(
    asset: &Asset,
    version: &str,
    drafted_at: chrono::DateTime<Utc>,
) -> (AssetRelease, BuildRun) {
    let mut release = AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse(version).expect("release version"),
        GitCommitSha::parse("a".repeat(40)).expect("commit SHA"),
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("manifest digest"),
        drafted_at,
    )
    .expect("draft release");
    let build =
        succeeded_hosted_agent_build(asset.organization_id, asset.id, release.id, drafted_at);
    let outcome = project_hosted_build_outcome(&build)
        .expect("project hosted outcome")
        .expect("successful hosted outcome");
    release
        .publish_from_hosted_build(asset, &outcome)
        .expect("publish release");
    (release, build)
}

fn published_skill_release(
    asset: &Asset,
    version: &str,
    character: char,
    drafted_at: chrono::DateTime<Utc>,
) -> AssetRelease {
    let mut release = AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse(version).expect("Skill release version"),
        GitCommitSha::parse(character.to_string().repeat(40)).expect("Skill commit SHA"),
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
            .expect("Skill manifest digest"),
        drafted_at,
    )
    .expect("draft Skill release");
    release
        .publish_skill(
            asset,
            AssetReleaseArtifact::skill_bundle(
                Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
                    .expect("Skill bundle digest"),
                1024,
            )
            .expect("Skill bundle artifact"),
            drafted_at + Duration::milliseconds(1),
        )
        .expect("publish Skill release");
    release
}

fn source_template() -> SourceWorkloadTemplate {
    SourceWorkloadTemplate {
        process: ServiceProcess {
            command: Vec::new(),
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 33_554_432,
            pids: 32,
            ephemeral_storage_bytes: Some(64 * 1024 * 1024),
        },
        ports: Vec::new(),
        health: None,
    }
}

async fn activate(
    workloads: &InMemoryWorkloadRepository,
    organization_id: OrganizationId,
    deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
    node_id: NodeId,
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
            node_id,
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

struct TestNodePoolRepository {
    pool: NodePool,
}

#[async_trait]
impl INodePoolRepository for TestNodePoolRepository {
    async fn replay(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<NodePool>, RepositoryError> {
        Ok(None)
    }

    async fn save(
        &self,
        _write: NodePoolWrite,
    ) -> Result<IdempotentWrite<NodePool>, RepositoryError> {
        Err(RepositoryError::Storage("unused node pool write".into()))
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        pool_id: NodePoolId,
    ) -> Result<NodePool, RepositoryError> {
        if self.pool.organization_id == organization_id && self.pool.id == pool_id {
            Ok(self.pool.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<NodePool>, RepositoryError> {
        Ok((self.pool.organization_id == organization_id)
            .then(|| self.pool.clone())
            .into_iter()
            .collect())
    }
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
