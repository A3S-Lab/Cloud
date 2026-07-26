use super::*;

pub(super) struct Fixture {
    pub(super) organization_id: OrganizationId,
    pub(super) build_id: BuildRunId,
    pub(super) run_id: String,
    pub(super) node_id: NodeId,
    pub(super) agent_instance_id: Uuid,
    pub(super) apply_sequence: u64,
}

pub(super) async fn create_fixture(
    postgres_url: &str,
    executor: &PostgresExecutor,
    paths: &ProbePaths,
    revision: ExternalSourceRevision,
    input_artifact: BuildArtifact,
    runtime_output: BuildArtifact,
) -> Result<Fixture, Box<dyn Error>> {
    let now = Utc::now();
    let organization_id = revision.organization_id;
    let project_id = revision.project_id;
    let environment_id = revision.environment_id;
    let source_revision_id = revision.id;
    let identity = PostgresIdentityRepository::new(executor.clone());
    IOrganizationRepository::create(
        &identity,
        Organization::create(
            organization_id,
            OrganizationName::parse("G0 process death")?,
            now,
        ),
        event(
            "organization.created",
            organization_id,
            organization_id.as_uuid(),
        ),
        idempotency("g0.organization", organization_id.as_uuid())?,
    )
    .await?;
    let projects = PostgresProjectsRepository::new(executor.clone());
    IProjectRepository::create(
        &projects,
        Project::create(
            organization_id,
            project_id,
            ProjectName::parse("G0 process death")?,
            now,
        ),
        event("project.created", organization_id, project_id.as_uuid()),
        idempotency("g0.project", project_id.as_uuid())?,
    )
    .await?;
    IEnvironmentRepository::create(
        &projects,
        Environment::create(
            organization_id,
            project_id,
            environment_id,
            EnvironmentName::parse("G0 process death")?,
            now,
        ),
        event(
            "environment.created",
            organization_id,
            environment_id.as_uuid(),
        ),
        idempotency("g0.environment", environment_id.as_uuid())?,
    )
    .await?;
    let sources = PostgresSourceRevisionRepository::new(executor.clone());
    sources
        .accept(AcceptSourceRevision {
            revision,
            webhook_delivery: None,
            idempotency: idempotency("g0.source", source_revision_id.as_uuid())?,
            event: event(
                "source.revision.accepted",
                organization_id,
                source_revision_id.as_uuid(),
            ),
        })
        .await?;
    let builds = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let build = builds
        .reserve_pending(1, now)
        .await?
        .pop()
        .ok_or("G0 process-death fixture did not reserve a BuildRun")?;
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let (node_id, agent_instance_id) =
        ready_node(nodes.as_ref(), organization_id, now, build_capabilities()).await?;
    let input: Arc<dyn IBuildInputPreparer> = Arc::new(StoredInputPreparer {
        artifact: input_artifact,
    });
    let runtime = runtime(
        executor,
        paths,
        builds,
        Arc::new(RejectingPublisher),
        Arc::new(RejectingEvidenceGenerator),
        input,
    )?;
    let flow = connect_flow(postgres_url, Arc::new(runtime)).await?;
    let run_id = build.operation_id.to_string();
    flow.engine()
        .start_with_id(
            run_id.clone(),
            workflow_spec(),
            flow_input(organization_id, build.id),
        )
        .await?;
    require(
        flow.engine().snapshot(&run_id).await?.status == WorkflowRunStatus::Suspended,
        "G0 process-death fixture did not wait for Runtime output",
    )?;
    let apply = lease_single_command(nodes.as_ref(), node_id, agent_instance_id, 0).await?;
    let NodeCommandPayload::RuntimeApply { request, .. } = &apply.payload else {
        return Err("G0 process-death fixture did not dispatch Runtime apply".into());
    };
    let observation = succeeded_observation(&request.spec, &runtime_output)?;
    record_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        build_capabilities(),
        &apply,
        observation.clone(),
    )
    .await?;
    acknowledge_apply(nodes.as_ref(), &apply, observation).await?;
    Ok(Fixture {
        organization_id,
        build_id: build.id,
        run_id,
        node_id,
        agent_instance_id,
        apply_sequence: apply.sequence,
    })
}
