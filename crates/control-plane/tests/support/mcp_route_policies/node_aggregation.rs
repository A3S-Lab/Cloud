use super::*;
use a3s_cloud_control_plane::modules::edge::domain::events::GatewayScopeCreated;
use a3s_cloud_control_plane::modules::edge::CreateGatewayScopeWrite;
use a3s_cloud_control_plane::modules::projects::domain::entities::{Environment, Project};
use a3s_cloud_control_plane::modules::projects::domain::events::{
    EnvironmentCreated, ProjectCreated,
};
use a3s_cloud_control_plane::modules::projects::domain::repositories::{
    IEnvironmentRepository, IProjectRepository,
};
use a3s_cloud_control_plane::modules::projects::domain::value_objects::{
    EnvironmentName, ProjectName,
};
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;

pub(super) struct Fixture<'a> {
    pub executor: &'a PostgresExecutor,
    pub edge: &'a PostgresEdgeRepository,
    pub assets: &'a PostgresAssetRepository,
    pub workloads: &'a PostgresWorkloadRepository,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub scope: &'a a3s_cloud_control_plane::modules::edge::GatewayScope,
    pub policy: &'a McpRoutePolicy,
    pub profile: &'a McpServiceProfile,
    pub asset: &'a Asset,
    pub release: &'a AssetRelease,
    pub profile_binding: &'a McpServiceProfileBinding,
    pub workload_id: WorkloadId,
    pub workload_revision: &'a WorkloadRevision,
    pub observed_at: DateTime<Utc>,
}

pub(super) async fn exercise(fixture: Fixture<'_>) -> TestResult {
    fail_pending_snapshot(&fixture).await?;

    let first_credential_at = fixture.observed_at + Duration::milliseconds(1);
    let first_credential = create_credential(
        fixture.edge,
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        "a3s_mcp_1122334455667788",
        first_credential_at,
    )
    .await?;
    let first_policy_at = fixture.observed_at + Duration::milliseconds(2);
    let mut first_policy = (*fixture.policy).clone();
    let mut first_spec = first_policy.spec().clone();
    first_spec.expires_at = fixture.observed_at + Duration::hours(1);
    replace_grant_credential(&mut first_spec, &first_credential)?;
    assert!(first_policy.revise(first_spec, fixture.profile, first_policy_at)?);
    assert_eq!(
        fixture
            .edge
            .update_mcp_route_policy(first_policy.clone(), fixture.policy.policy_revision())
            .await?,
        first_policy
    );

    let second_project_at = fixture.observed_at + Duration::milliseconds(3);
    let (second_project_id, second_environment_id) =
        create_tenant_environment(fixture.executor, fixture.organization_id, second_project_at)
            .await?;
    let second_scope_at = fixture.observed_at + Duration::milliseconds(5);
    let second_scope = create_scope(
        fixture.edge,
        fixture.organization_id,
        second_project_id,
        second_environment_id,
        fixture.scope.node_id,
        second_scope_at,
    )
    .await?;
    let second_credential = create_credential(
        fixture.edge,
        fixture.organization_id,
        second_project_id,
        second_environment_id,
        "a3s_mcp_8877665544332211",
        fixture.observed_at + Duration::milliseconds(6),
    )
    .await?;
    let second_workload = create_active_workload(
        &fixture,
        second_project_id,
        second_environment_id,
        &second_scope,
        fixture.observed_at + Duration::milliseconds(7),
    )
    .await?;
    let (second_claim, second_hostname) = create_verified_domain_claim(
        fixture.edge,
        fixture.organization_id,
        second_project_id,
        second_environment_id,
        fixture.observed_at + Duration::milliseconds(20),
    )
    .await?;
    let second_policy_at = fixture.observed_at + Duration::milliseconds(23);
    let mut second_spec = first_policy.spec().clone();
    second_spec.route_id = RouteId::new();
    second_spec.project_id = second_project_id;
    second_spec.environment_id = second_environment_id;
    second_spec.gateway_scope_id = second_scope.id;
    second_spec.domain_claim_id = second_claim.id;
    second_spec.workload_id = second_workload.workload_id;
    second_spec.hostname = second_hostname.clone();
    replace_grant_credential(&mut second_spec, &second_credential)?;
    let second_policy = McpRoutePolicy::create(second_spec, fixture.profile, second_policy_at)?;
    assert_eq!(
        fixture
            .edge
            .create_mcp_route_policy(second_policy.clone())
            .await?,
        second_policy
    );

    let planned_at = fixture.observed_at + Duration::milliseconds(25);
    let scope_set = fixture
        .edge
        .mcp_gateway_reconciliation_scope_set(fixture.scope.node_id, planned_at)
        .await?;
    let mut expected_scope_ids = vec![fixture.scope.id, second_scope.id];
    expected_scope_ids.sort();
    assert_eq!(
        scope_set.iter().map(|scope| scope.id).collect::<Vec<_>>(),
        expected_scope_ids
    );

    let partial_stage = plan_gateway_snapshot(
        fixture.edge,
        fixture.assets,
        fixture.workloads,
        fixture.scope,
        fixture.workload_id,
        planned_at,
    )
    .await?;
    assert_eq!(
        stage_artifact_counts(fixture.executor, &partial_stage).await?,
        (0, 0, 0, 0)
    );
    assert!(matches!(
        fixture
            .edge
            .stage_mcp_gateway_snapshot(partial_stage.clone())
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        stage_artifact_counts(fixture.executor, &partial_stage).await?,
        (0, 0, 0, 0)
    );

    let database = Database::new(PostgresDialect, (*fixture.executor).clone());
    let publications_before = publication_count(&database, fixture.scope.node_id).await?;
    let shared_edge = Arc::new((*fixture.edge).clone());
    let inputs = Arc::new(McpRouteProjectionInputReader::new(
        shared_edge.clone(),
        shared_edge.clone(),
        Arc::new((*fixture.assets).clone()),
        Arc::new((*fixture.workloads).clone()),
    ));
    let target_reader = FixtureRouteTargetReader::single(fixture.workload_id)
        .with_revision(second_workload.revision_id, second_workload.workload_id);
    let route_planner =
        McpRouteProjectionPlanner::new(Arc::new(target_reader), McpRouteTargetProjectionCompiler);
    let scope_planner = Arc::new(McpGatewayProjectionSetPlanner::new(
        inputs,
        McpGatewayProjectionPlanner::new(route_planner, shared_edge.clone()),
        McpGatewayProjectionAssembler,
    ));
    let node_planner = Arc::new(McpGatewayNodeProjectionPlanner::new(
        scope_planner,
        McpGatewayProjectionAssembler,
    ));
    let reconciler = McpGatewayDesiredStateReconciler::new(
        shared_edge,
        node_planner,
        fixture_gateway_snapshot_compiler()?,
        std::time::Duration::from_secs(60),
        Duration::minutes(5),
        Duration::hours(1),
        Duration::minutes(5),
        Duration::minutes(5),
        100,
    )?;
    let report = reconciler.run_once(planned_at).await?;
    assert_eq!(report.scopes, 2);
    assert_eq!(report.gateway_members, 1);
    assert_eq!(report.pending_publications, 0);
    assert_eq!(report.staged_snapshots, 1);
    assert_eq!(report.unchanged_snapshots, 0);
    assert!(report.failures.is_empty());
    assert_eq!(
        publication_count(&database, fixture.scope.node_id).await?,
        publications_before + 1
    );

    let (gateway_revision, route_count, acl) = database
        .fetch_one_as(
            sql_query::<(u64, u32, String)>(
                "select marker.gateway_revision, marker.mcp_route_count, publication.acl \
                 from mcp_gateway_snapshot_publications marker \
                 join gateway_publications publication \
                   on publication.node_id = marker.node_id \
                  and publication.revision = marker.gateway_revision \
                 where marker.node_id = ",
            )
            .bind(fixture.scope.node_id.as_uuid())
            .append(" order by marker.gateway_revision desc limit 1"),
        )
        .await?;
    assert_eq!(route_count, 2);
    assert!(acl.contains(first_policy.spec().hostname.as_str()));
    assert!(acl.contains(second_hostname.as_str()));

    let scope_rows = database
        .fetch_all_as(
            sql_query::<(Uuid, u32)>(
                "select gateway_scope_id, mcp_route_count \
                 from mcp_gateway_snapshot_publication_scopes where node_id = ",
            )
            .bind(fixture.scope.node_id.as_uuid())
            .append(" and gateway_revision = ")
            .bind(gateway_revision)
            .append(" order by gateway_scope_id asc"),
        )
        .await?
        .rows;
    assert_eq!(
        scope_rows,
        expected_scope_ids
            .into_iter()
            .map(|scope_id| (scope_id.as_uuid(), 1))
            .collect::<Vec<_>>()
    );
    Ok(())
}

async fn fail_pending_snapshot(fixture: &Fixture<'_>) -> TestResult {
    let pending = fixture
        .edge
        .pending_mcp_gateway_snapshots(100)
        .await?
        .into_iter()
        .filter(|target| target.publication.node_id == fixture.scope.node_id)
        .max_by_key(|target| target.publication.revision)
        .ok_or("MCP node aggregation fixture expected the pending cleanup snapshot")?;
    fixture
        .edge
        .mark_mcp_gateway_snapshot_unavailable(
            pending.organization_id,
            pending.gateway_scope_id,
            pending.publication.node_id,
            pending.publication.revision,
            pending.publication.command_id,
            "complete the single-scope fixture before node aggregation",
            fixture.observed_at,
        )
        .await?;
    Ok(())
}

async fn create_tenant_environment(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    created_at: DateTime<Utc>,
) -> TestResult<(ProjectId, EnvironmentId)> {
    let projects = PostgresProjectsRepository::new((*executor).clone());
    let project = Project::create(
        organization_id,
        ProjectId::new(),
        ProjectName::parse("MCP node aggregation")?,
        created_at,
    );
    IProjectRepository::create(
        &projects,
        project.clone(),
        ProjectCreated::envelope(&project, Uuid::now_v7())?,
        idempotency(
            organization_id,
            "projects",
            "postgres-mcp-node-aggregation-project",
            b"mcp-node-aggregation-project",
        )?,
    )
    .await?;
    let environment = Environment::create(
        organization_id,
        project.id,
        EnvironmentId::new(),
        EnvironmentName::parse("aggregation")?,
        created_at + Duration::milliseconds(1),
    );
    IEnvironmentRepository::create(
        &projects,
        environment.clone(),
        EnvironmentCreated::envelope(&environment, Uuid::now_v7())?,
        idempotency(
            organization_id,
            format!("projects/{}/environments", project.id),
            "postgres-mcp-node-aggregation-environment",
            b"mcp-node-aggregation-environment",
        )?,
    )
    .await?;
    Ok((project.id, environment.id))
}

async fn create_scope(
    edge: &PostgresEdgeRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    node_id: NodeId,
    created_at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::edge::GatewayScope> {
    let scope = a3s_cloud_control_plane::modules::edge::GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        node_id,
        created_at,
    )?;
    edge.create_gateway_scope(CreateGatewayScopeWrite {
        scope: scope.clone(),
        idempotency: idempotency(
            organization_id,
            format!("projects/{project_id}/environments/{environment_id}/gateway-scopes"),
            "postgres-mcp-node-aggregation-scope",
            scope.id.to_string().as_bytes(),
        )?,
        event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
    })
    .await?;
    Ok(scope)
}

async fn create_credential(
    edge: &PostgresEdgeRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    prefix: &str,
    created_at: DateTime<Utc>,
) -> TestResult<McpCredential> {
    let credential = McpCredential::issue(
        McpCredentialId::new(),
        organization_id,
        project_id,
        environment_id,
        prefix,
        VERIFIER,
        created_at + Duration::days(30),
        created_at,
    )?;
    assert_eq!(
        edge.create_mcp_credential(credential.clone()).await?,
        credential
    );
    Ok(credential)
}

struct ActiveWorkload {
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
}

async fn create_active_workload(
    fixture: &Fixture<'_>,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    scope: &a3s_cloud_control_plane::modules::edge::GatewayScope,
    created_at: DateTime<Utc>,
) -> TestResult<ActiveWorkload> {
    let workload = Workload::create(
        WorkloadId::new(),
        fixture.organization_id,
        project_id,
        environment_id,
        ResourceName::parse("Aggregated MCP runtime")?,
        created_at,
    );
    let mut revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        fixture.workload_revision.resolved_template()?.clone(),
        created_at,
    )?;
    revision.bind_mcp_release(
        &workload,
        fixture.asset,
        fixture.release,
        fixture.profile_binding,
    )?;
    let deployment = Deployment::create(
        DeploymentId::new(),
        fixture.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        created_at,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        fixture.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)?,
        json!({
            "deploymentId": deployment.id,
            "mcpAssetReleaseId": fixture.release.id,
            "mcpProfileDigest": fixture.profile.digest(),
            "revisionId": revision.id,
            "workloadId": workload.id,
        }),
        created_at,
    );
    fixture
        .workloads
        .create_deployment(CreateDeploymentBundle {
            workload: workload.clone(),
            control: WorkloadControlSpec::unmanaged_single_replica(),
            revision: revision.clone(),
            deployment: deployment.clone(),
            operation,
            idempotency: idempotency(
                fixture.organization_id,
                "workloads",
                "postgres-mcp-node-aggregation-workload",
                revision.request_digest.as_bytes(),
            )?,
            event: DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?,
        })
        .await?;
    let resolving = fixture
        .workloads
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            created_at + Duration::milliseconds(1),
        )
        .await?;
    let scheduled = fixture
        .workloads
        .assign_node(
            deployment.id,
            resolving.aggregate_version,
            scope.node_id,
            created_at + Duration::milliseconds(2),
        )
        .await?;
    let command_id = NodeCommandId::from_uuid(deployment.id.as_uuid());
    let command_deadline = scheduled.updated_at + Duration::minutes(5);
    let runtime_spec = project_runtime_spec(&revision)?;
    let command = PostgresNodeRepository::new((*fixture.executor).clone())
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: scope.node_id,
            aggregate_id: deployment.workload_id.as_uuid(),
            payload: NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("deployment:{}:apply", deployment.id),
                    deadline_at_ms: Some(u64::try_from(command_deadline.timestamp_millis())?),
                    spec: runtime_spec,
                }),
                resource_claim: None,
            },
            issued_at: scheduled.updated_at,
            not_after: command_deadline,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await?
        .value;
    let applying = fixture
        .workloads
        .mark_dispatched(
            deployment.id,
            scheduled.aggregate_version,
            command.id,
            created_at + Duration::milliseconds(3),
        )
        .await?;
    let verifying = fixture
        .workloads
        .mark_verifying(
            deployment.id,
            applying.aggregate_version,
            created_at + Duration::milliseconds(4),
        )
        .await?;
    fixture
        .workloads
        .activate(
            deployment.id,
            verifying.aggregate_version,
            false,
            created_at + Duration::milliseconds(5),
        )
        .await?;
    Ok(ActiveWorkload {
        workload_id: workload.id,
        revision_id: revision.id,
    })
}

async fn create_verified_domain_claim(
    edge: &PostgresEdgeRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    created_at: DateTime<Utc>,
) -> TestResult<(DomainClaim, RouteHostname)> {
    let hostname = RouteHostname::parse("mcp-node-aggregation.integration.example")?;
    let mut claim = DomainClaim::create(
        DomainClaimId::new(),
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse(hostname.as_str())?,
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        created_at,
    )?;
    edge.create_domain_claim(CreateDomainClaimWrite {
        claim: claim.clone(),
        idempotency: idempotency(
            organization_id,
            "domain-claims",
            "postgres-mcp-node-aggregation-domain",
            hostname.as_str().as_bytes(),
        )?,
        event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
    })
    .await?;
    let expected_version = claim.aggregate_version;
    claim.verify(created_at + Duration::milliseconds(1))?;
    edge.transition_domain_claim(TransitionDomainClaim {
        claim: claim.clone(),
        expected_version,
        idempotency: idempotency(
            organization_id,
            format!("domain-claims/{}", claim.id),
            "postgres-mcp-node-aggregation-domain-verification",
            b"verified",
        )?,
        event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
    })
    .await?;
    Ok((claim, hostname))
}

fn replace_grant_credential(
    spec: &mut McpRoutePolicySpec,
    credential: &McpCredential,
) -> Result<(), String> {
    if spec.grants.is_empty() {
        return Err("MCP node aggregation fixture requires at least one grant".into());
    }
    for grant in &mut spec.grants {
        grant.credential_id = credential.id.as_uuid();
        grant.credential_generation = credential.generation();
    }
    Ok(())
}

async fn publication_count(
    database: &Database<PostgresDialect, PostgresExecutor>,
    node_id: NodeId,
) -> TestResult<i64> {
    Ok(database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from gateway_publications where node_id = ")
                .bind(node_id.as_uuid()),
        )
        .await?)
}
