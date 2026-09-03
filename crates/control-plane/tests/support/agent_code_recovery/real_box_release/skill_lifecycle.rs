use super::*;

/// Drive the immutable Skill binding commands through the production Workload,
/// Flow, Fleet, Runtime, and Box path.  Every revision is applied to the same
/// enrolled node so the gate proves that the Skill mount changes only with the
/// persisted Workload revision and that older Runtime units are retired before
/// their claims are released.
#[allow(clippy::too_many_arguments)]
pub(super) async fn exercise_skill_binding_lifecycle(
    assets: Arc<PostgresAssetRepository>,
    workloads: Arc<PostgresWorkloadRepository>,
    secrets: Arc<PostgresSecretRepository>,
    nodes: Arc<PostgresNodeRepository>,
    coordinator: &FlowOperationCoordinator,
    operations: Arc<dyn IOperationRepository>,
    executor_on_node: &CommandExecutor,
    runtime: &dyn RuntimeClient,
    transport: &PublishedManifestTransport,
    secret_transport: &PublishedAgentSecretTransport,
    capabilities: &RuntimeCapabilities,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    node_id: NodeId,
    agent_instance_id: Uuid,
    provider_secret_id: SecretId,
    signing_secret_id: SecretId,
    initial_revision: WorkloadRevision,
    initial_spec: RuntimeUnitSpec,
    initial_sequence: u64,
    initial_operation_id: OperationId,
    home: &Path,
    node_state: &Path,
) -> TestResult {
    let fixture_at = canonical_timestamp(Utc::now());
    let skill_asset = create_skill_asset(assets.as_ref(), organization_id, fixture_at).await?;
    let first_archive = skill_bundle_archive("A3S skill release one\n")?;
    let second_archive = skill_bundle_archive("A3S skill release two\n")?;
    let first_size_bytes = first_archive.len() as u64;
    let second_size_bytes = second_archive.len() as u64;
    let first_artifact = skill_bundle_artifact(&first_archive)?;
    let second_artifact = skill_bundle_artifact(&second_archive)?;
    transport.register_skill_artifact(first_artifact.clone(), first_archive)?;
    transport.register_skill_artifact(second_artifact.clone(), second_archive)?;
    let first_release = create_skill_release(
        assets.as_ref(),
        &skill_asset,
        "1.0.0",
        'c',
        &first_artifact,
        first_size_bytes,
        fixture_at + Duration::milliseconds(1),
        "skill-real-box-release-one",
    )
    .await?;
    let second_release = create_skill_release(
        assets.as_ref(),
        &skill_asset,
        "2.0.0",
        'd',
        &second_artifact,
        second_size_bytes,
        fixture_at + Duration::milliseconds(2),
        "skill-real-box-release-two",
    )
    .await?;

    let bind_handler =
        BindSkillWorkloadDeploymentHandler::new(assets.clone(), workloads.clone(), secrets.clone());
    let unbind_handler =
        UnbindSkillWorkloadDeploymentHandler::new(workloads.clone(), secrets.clone());
    let rollback_handler = RollbackWorkloadDeploymentHandler::new(workloads.clone(), secrets);
    let access = workload_organization_access_for_conformance();
    let mut requested_at = canonical_timestamp(Utc::now())
        .max(initial_revision.created_at)
        .max(second_release.updated_at);
    let mut current_spec = initial_spec;
    let mut current_sequence = initial_sequence;
    let mut current_operation_id = initial_operation_id;
    let mut retired: Vec<(RuntimeUnitSpec, OperationId)> = Vec::new();

    let bind_one = bind_handler
        .execute(
            BindSkillWorkloadDeployment {
                organization_id,
                workload_id,
                access: access.clone(),
                skill_asset_id: skill_asset.id,
                skill_asset_release_id: first_release.id,
                idempotency_key: "skill-real-box-bind-one".into(),
                request_id: Uuid::now_v7(),
                requested_at: next_revision_time(&mut requested_at),
            },
            context(),
        )
        .await?
        .map_err(|error| invalid(format!("could not bind first Skill release: {error}")))?;
    let first_bound_spec = project_runtime_spec(&bind_one.bundle.revision).map_err(invalid)?;
    verify_skill_runtime_spec(&first_bound_spec, &skill_asset, Some(&first_artifact))?;
    secret_transport.bind_revision(bind_one.bundle.revision.id.as_uuid())?;
    retired.push((current_spec.clone(), current_operation_id));
    current_sequence = transition_skill_revision(
        coordinator,
        &nodes,
        workloads.as_ref(),
        operations.as_ref(),
        executor_on_node,
        runtime,
        capabilities,
        node_id,
        agent_instance_id,
        organization_id,
        current_sequence,
        &current_spec,
        &first_bound_spec,
        bind_one.bundle.deployment.id,
        bind_one.bundle.operation.id,
        &skill_asset,
        Some("A3S skill release one\n"),
    )
    .await?;
    current_spec = first_bound_spec;
    current_operation_id = bind_one.bundle.operation.id;

    let rebound = bind_handler
        .execute(
            BindSkillWorkloadDeployment {
                organization_id,
                workload_id,
                access: access.clone(),
                skill_asset_id: skill_asset.id,
                skill_asset_release_id: second_release.id,
                idempotency_key: "skill-real-box-bind-two".into(),
                request_id: Uuid::now_v7(),
                requested_at: next_revision_time(&mut requested_at),
            },
            context(),
        )
        .await?
        .map_err(|error| invalid(format!("could not rebind second Skill release: {error}")))?;
    let second_bound_spec = project_runtime_spec(&rebound.bundle.revision).map_err(invalid)?;
    verify_skill_runtime_spec(&second_bound_spec, &skill_asset, Some(&second_artifact))?;
    secret_transport.bind_revision(rebound.bundle.revision.id.as_uuid())?;
    retired.push((current_spec.clone(), current_operation_id));
    current_sequence = transition_skill_revision(
        coordinator,
        &nodes,
        workloads.as_ref(),
        operations.as_ref(),
        executor_on_node,
        runtime,
        capabilities,
        node_id,
        agent_instance_id,
        organization_id,
        current_sequence,
        &current_spec,
        &second_bound_spec,
        rebound.bundle.deployment.id,
        rebound.bundle.operation.id,
        &skill_asset,
        Some("A3S skill release two\n"),
    )
    .await?;
    current_spec = second_bound_spec;
    current_operation_id = rebound.bundle.operation.id;

    let unbound = unbind_handler
        .execute(
            UnbindSkillWorkloadDeployment {
                organization_id,
                workload_id,
                access: access.clone(),
                skill_asset_id: skill_asset.id,
                idempotency_key: "skill-real-box-unbind".into(),
                request_id: Uuid::now_v7(),
                requested_at: next_revision_time(&mut requested_at),
            },
            context(),
        )
        .await?
        .map_err(|error| invalid(format!("could not unbind Skill release: {error}")))?;
    let unbound_spec = project_runtime_spec(&unbound.bundle.revision).map_err(invalid)?;
    verify_skill_runtime_spec(&unbound_spec, &skill_asset, None)?;
    secret_transport.bind_revision(unbound.bundle.revision.id.as_uuid())?;
    retired.push((current_spec.clone(), current_operation_id));
    current_sequence = transition_skill_revision(
        coordinator,
        &nodes,
        workloads.as_ref(),
        operations.as_ref(),
        executor_on_node,
        runtime,
        capabilities,
        node_id,
        agent_instance_id,
        organization_id,
        current_sequence,
        &current_spec,
        &unbound_spec,
        unbound.bundle.deployment.id,
        unbound.bundle.operation.id,
        &skill_asset,
        None,
    )
    .await?;
    current_spec = unbound_spec;
    current_operation_id = unbound.bundle.operation.id;

    let rollback_source = bind_one.bundle.revision.id;
    let rollback = rollback_handler
        .execute(
            RollbackWorkloadDeployment {
                organization_id,
                workload_id,
                access,
                source_revision_id: rollback_source,
                idempotency_key: "skill-real-box-rollback".into(),
                request_id: Uuid::now_v7(),
                requested_at: next_revision_time(&mut requested_at),
            },
            context(),
        )
        .await?
        .map_err(|error| {
            invalid(format!(
                "could not roll back to first Skill release: {error}"
            ))
        })?;
    let rollback_spec = project_runtime_spec(&rollback.bundle.revision).map_err(invalid)?;
    verify_skill_runtime_spec(&rollback_spec, &skill_asset, Some(&first_artifact))?;
    secret_transport.bind_revision(rollback.bundle.revision.id.as_uuid())?;
    retired.push((current_spec.clone(), current_operation_id));
    current_sequence = transition_skill_revision(
        coordinator,
        &nodes,
        workloads.as_ref(),
        operations.as_ref(),
        executor_on_node,
        runtime,
        capabilities,
        node_id,
        agent_instance_id,
        organization_id,
        current_sequence,
        &current_spec,
        &rollback_spec,
        rollback.bundle.deployment.id,
        rollback.bundle.operation.id,
        &skill_asset,
        Some("A3S skill release one\n"),
    )
    .await?;
    current_spec = rollback_spec;

    let revisions = workloads
        .list_revisions(organization_id, workload_id)
        .await?;
    if revisions.len() != 5
        || revisions
            .iter()
            .map(|revision| revision.generation)
            .collect::<Vec<_>>()
            != [5, 4, 3, 2, 1]
        || revisions
            .iter()
            .find(|revision| revision.generation == 2)
            .and_then(|revision| revision.skill_binding(skill_asset.id))
            .is_none_or(|binding| binding.asset_release_id() != first_release.id)
        || revisions
            .iter()
            .find(|revision| revision.generation == 3)
            .and_then(|revision| revision.skill_binding(skill_asset.id))
            .is_none_or(|binding| binding.asset_release_id() != second_release.id)
        || revisions
            .iter()
            .find(|revision| revision.generation == 4)
            .is_none_or(|revision| revision.skill_binding(skill_asset.id).is_some())
        || revisions
            .iter()
            .find(|revision| revision.generation == 5)
            .and_then(|revision| revision.skill_binding(skill_asset.id))
            .is_none_or(|binding| binding.asset_release_id() != first_release.id)
    {
        return Err(
            invalid("Skill Workload revision history changed during real Box lifecycle").into(),
        );
    }

    let stop_operation_id = request_workload_stop(
        workloads.as_ref(),
        organization_id,
        workload_id,
        next_revision_time(&mut requested_at),
    )
    .await?;
    let stop = next_flow_command(
        coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        current_sequence,
        LifecycleCommandKind::RuntimeStop,
    )
    .await?;
    let stop_ack = execute_and_persist(
        executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        capabilities,
        &stop,
    )
    .await?;
    verify_stopped_acknowledgement(&stop_ack, &current_spec)?;
    let release = next_flow_command(
        coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        stop.sequence,
        LifecycleCommandKind::ResourceRelease,
    )
    .await?;
    execute_and_persist(
        executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        capabilities,
        &release,
    )
    .await?;
    drive_until_stopped(
        coordinator,
        workloads.as_ref(),
        operations.as_ref(),
        organization_id,
        workload_id,
        stop_operation_id,
    )
    .await?;

    let current_remove = enqueue_remove(
        nodes.as_ref(),
        node_id,
        workload_id.as_uuid(),
        stop_operation_id.as_uuid(),
        &current_spec,
    )
    .await?;
    let current_remove_command =
        lease_only_command(nodes.as_ref(), node_id, agent_instance_id, release.sequence).await?;
    if current_remove_command.command_id != current_remove.id.as_uuid() {
        return Err(invalid("Fleet changed the current Skill Runtime removal").into());
    }
    let current_remove_ack = execute_and_persist(
        executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        capabilities,
        &current_remove_command,
    )
    .await?;
    verify_removed_acknowledgement(&current_remove_ack, &current_spec)?;
    current_sequence = current_remove_command.sequence;
    if !matches!(
        runtime.inspect(&current_spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid("rolled-back Skill Runtime remained after removal").into());
    }

    for (retired_spec, retired_operation_id) in retired {
        let remove = enqueue_remove(
            nodes.as_ref(),
            node_id,
            workload_id.as_uuid(),
            retired_operation_id.as_uuid(),
            &retired_spec,
        )
        .await?;
        let command =
            lease_only_command(nodes.as_ref(), node_id, agent_instance_id, current_sequence)
                .await?;
        if command.command_id != remove.id.as_uuid() {
            return Err(invalid("Fleet changed a retired Skill Runtime removal").into());
        }
        let acknowledgement = execute_and_persist(
            executor_on_node,
            &nodes,
            node_id,
            agent_instance_id,
            capabilities,
            &command,
        )
        .await?;
        verify_removed_acknowledgement(&acknowledgement, &retired_spec)?;
        current_sequence = command.sequence;
        if !matches!(
            runtime.inspect(&retired_spec.unit_id).await?,
            RuntimeInspection::NotFound { .. }
        ) {
            return Err(invalid("retired Skill Runtime remained after removal").into());
        }
    }

    if transport.downloads.load(Ordering::SeqCst) != 1 || transport.skill_downloads() != 2 {
        return Err(invalid(format!(
            "real Box Skill Artifact downloads were not content-addressed: manifest={} skill={}",
            transport.downloads.load(Ordering::SeqCst),
            transport.skill_downloads()
        ))
        .into());
    }
    if secret_transport.calls(provider_secret_id)? < 5
        || secret_transport.calls(signing_secret_id)? < 5
    {
        return Err(invalid(
            "real Box Skill lifecycle did not materialize Agent Secrets for every revision",
        )
        .into());
    }
    verify_clean_state(home, node_state)?;
    println!(
        "A3S_CLOUD_A0_5_REAL_BOX_SKILL_LIFECYCLE_CERTIFIED provider=postgresql/a3s-box bind=release1 rebind=release2 unbind=none rollback=release1 revisions=5 manifest_downloads=1 skill_downloads=2 cleanup=removed"
    );
    Ok(())
}

fn next_revision_time(last: &mut DateTime<Utc>) -> DateTime<Utc> {
    let now = canonical_timestamp(Utc::now());
    let next = now.max(*last + Duration::milliseconds(1));
    *last = next;
    next
}

async fn create_skill_asset(
    assets: &PostgresAssetRepository,
    organization_id: OrganizationId,
    created_at: DateTime<Utc>,
) -> TestResult<Asset> {
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("A0.5 Real Box Skill")?,
        AssetKind::Skill,
        created_at,
    )?;
    assets
        .create_asset(CreateAssetWrite {
            asset: asset.clone(),
            event: AssetCreated::envelope(&asset, Uuid::now_v7())?,
            idempotency: idempotency(
                &format!("test.a0-5.organizations/{organization_id}/skills"),
                "create-real-box-skill",
                b"create-real-box-skill",
            )?,
        })
        .await?;
    Ok(asset)
}

fn skill_bundle_archive(payload: &str) -> TestResult<Vec<u8>> {
    let payload = payload.as_bytes();
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("SKILL.md")?;
    header.set_size(payload.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder.append(&header, payload)?;
    builder.finish()?;
    Ok(builder.into_inner()?)
}

fn skill_bundle_artifact(archive: &[u8]) -> TestResult<ArtifactRef> {
    let digest = Sha256Digest::from_bytes(archive);
    let artifact = ArtifactRef {
        uri: a3s_cloud_contracts::artifact_uri(digest.as_str())?,
        digest: digest.to_string(),
        media_type: SKILL_BUNDLE_MEDIA_TYPE.into(),
    };
    artifact.validate().map_err(invalid)?;
    Ok(artifact)
}

#[allow(clippy::too_many_arguments)]
async fn create_skill_release(
    assets: &PostgresAssetRepository,
    asset: &Asset,
    version: &str,
    commit_character: char,
    artifact: &ArtifactRef,
    size_bytes: u64,
    created_at: DateTime<Utc>,
    key: &str,
) -> TestResult<AssetRelease> {
    let release = AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse(version)?,
        GitCommitSha::parse(commit_character.to_string().repeat(40))?,
        Sha256Digest::from_bytes(format!("skill-manifest-{version}").as_bytes()),
        created_at,
    )?;
    assets
        .create_release(CreateAssetReleaseWrite {
            release: release.clone(),
            event: AssetReleaseDrafted::envelope(&release, Uuid::now_v7())?,
            hosted_build_requested_event: None,
            idempotency: idempotency(
                &format!(
                    "test.a0-5.organizations/{}/skills/releases",
                    asset.organization_id
                ),
                &format!("{key}-draft"),
                key.as_bytes(),
            )?,
        })
        .await?;
    let mut published = release;
    published.publish_skill(
        asset,
        AssetReleaseArtifact::skill_bundle(
            Sha256Digest::parse(artifact.digest.clone())?,
            size_bytes,
        )?,
        created_at + Duration::milliseconds(1),
    )?;
    let transitioned = assets
        .transition_release(TransitionAssetReleaseWrite {
            expected_aggregate_version: 1,
            event: AssetReleasePublished::envelope(&published, Uuid::now_v7())?,
            idempotency: idempotency(
                &format!(
                    "test.a0-5.organizations/{}/skills/releases",
                    asset.organization_id
                ),
                &format!("{key}-publish"),
                artifact.digest.as_bytes(),
            )?,
            release: published,
        })
        .await?;
    Ok(transitioned.release)
}

fn verify_skill_runtime_spec(
    spec: &RuntimeUnitSpec,
    asset: &Asset,
    expected: Option<&ArtifactRef>,
) -> TestResult {
    let mount_name = format!("skill-{}", asset.id);
    let mounts = spec
        .mounts
        .iter()
        .filter(|mount| mount.name == mount_name)
        .collect::<Vec<_>>();
    match expected {
        Some(artifact) => {
            if mounts.len() != 1 {
                return Err(
                    invalid("Skill-bound Runtime spec omitted its unique Skill mount").into(),
                );
            }
            let mount = mounts[0];
            if mount.target != format!("/a3s/skills/{}", asset.id)
                || !mount.read_only
                || !matches!(
                    &mount.source,
                    RuntimeMountSource::Artifact { artifact: mounted } if mounted == artifact
                )
            {
                return Err(
                    invalid("Skill-bound Runtime spec changed its typed read-only mount").into(),
                );
            }
        }
        None if !mounts.is_empty() => {
            return Err(invalid("unbound Runtime spec retained a Skill mount").into());
        }
        None => {}
    }
    Ok(())
}

async fn verify_skill_runtime(
    runtime: &dyn RuntimeClient,
    spec: &RuntimeUnitSpec,
    asset: &Asset,
    expected_payload: Option<&str>,
) -> TestResult {
    let target = format!("/a3s/skills/{}", asset.id);
    let command = match expected_payload {
        Some(payload) => {
            let line = payload.strip_suffix('\n').unwrap_or(payload);
            format!(
                "set -eu; printf '%s\\n' '{line}' | cmp - {target}/SKILL.md; if printf forbidden > {target}/forbidden 2>/dev/null; then exit 71; fi; test ! -e {target}/forbidden"
            )
        }
        None => format!("set -eu; test ! -e {target}"),
    };
    let result = runtime
        .exec(&RuntimeExecRequest {
            schema: RuntimeExecRequest::SCHEMA.into(),
            request_id: format!("a0-5-skill-runtime-check-{}", Uuid::now_v7()),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            command: vec!["/bin/sh".into(), "-c".into(), command],
            timeout_ms: 10_000,
            deadline_at_ms: None,
        })
        .await?;
    if result.exit_code != 0 {
        return Err(invalid(format!(
            "real Box Skill Runtime check failed: exit={} stderr={}",
            result.exit_code, result.stderr
        ))
        .into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn transition_skill_revision(
    coordinator: &FlowOperationCoordinator,
    nodes: &Arc<PostgresNodeRepository>,
    workloads: &PostgresWorkloadRepository,
    operations: &dyn IOperationRepository,
    executor_on_node: &CommandExecutor,
    runtime: &dyn RuntimeClient,
    capabilities: &RuntimeCapabilities,
    node_id: NodeId,
    agent_instance_id: Uuid,
    organization_id: OrganizationId,
    after_sequence: u64,
    previous_spec: &RuntimeUnitSpec,
    next_spec: &RuntimeUnitSpec,
    deployment_id: DeploymentId,
    operation_id: OperationId,
    skill_asset: &Asset,
    expected_payload: Option<&str>,
) -> TestResult<u64> {
    let prepare = next_flow_command(
        coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        after_sequence,
        LifecycleCommandKind::ResourcePrepare,
    )
    .await?;
    execute_and_persist(
        executor_on_node,
        nodes,
        node_id,
        agent_instance_id,
        capabilities,
        &prepare,
    )
    .await?;
    let apply = next_flow_command(
        coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        prepare.sequence,
        LifecycleCommandKind::RuntimeApply,
    )
    .await?;
    let NodeCommandPayload::RuntimeApply {
        request,
        resource_claim,
    } = &apply.payload
    else {
        return Err(invalid("Skill Workload Flow emitted a non-apply command").into());
    };
    if request.spec != *next_spec || resource_claim.is_none() {
        return Err(invalid("Skill Workload Flow changed the admitted Runtime spec").into());
    }
    let apply_ack = execute_and_persist(
        executor_on_node,
        nodes,
        node_id,
        agent_instance_id,
        capabilities,
        &apply,
    )
    .await?;
    let observation = applied_observation(&apply_ack)?.clone();
    verify_running_observation(&observation, next_spec)?;
    verify_skill_runtime(runtime, next_spec, skill_asset, expected_payload).await?;

    let retirement = next_flow_command(
        coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        apply.sequence,
        LifecycleCommandKind::RuntimeStop,
    )
    .await?;
    if let NodeCommandPayload::RuntimeStop { request } = &retirement.payload {
        if request.unit_id != previous_spec.unit_id
            || request.generation != previous_spec.generation
        {
            return Err(
                invalid("Skill Workload retirement changed the previous Runtime identity").into(),
            );
        }
    }
    let stop_ack = execute_and_persist(
        executor_on_node,
        nodes,
        node_id,
        agent_instance_id,
        capabilities,
        &retirement,
    )
    .await?;
    verify_stopped_acknowledgement(&stop_ack, previous_spec)?;
    let release = next_flow_command(
        coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        retirement.sequence,
        LifecycleCommandKind::ResourceRelease,
    )
    .await?;
    execute_and_persist(
        executor_on_node,
        nodes,
        node_id,
        agent_instance_id,
        capabilities,
        &release,
    )
    .await?;
    drive_until_active(
        coordinator,
        workloads,
        operations,
        organization_id,
        deployment_id,
        operation_id,
    )
    .await?;
    Ok(release.sequence)
}
