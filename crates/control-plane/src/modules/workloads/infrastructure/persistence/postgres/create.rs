use super::schema::{Deployments, WorkloadRevisionSkillBindings, WorkloadRevisions, Workloads};
use super::{operation_requests, queries, replicas};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use crate::modules::workloads::domain::entities::{DeploymentStatus, PlacementTopology, Workload};
use crate::modules::workloads::domain::repositories::{CreateDeploymentBundle, DeploymentBundle};
use a3s_orm::{insert_into, select_from, PostgresExecutor, PostgresTransaction};

pub(super) async fn deployment(
    executor: &PostgresExecutor,
    request: CreateDeploymentBundle,
) -> Result<DeploymentBundle, RepositoryError> {
    executor
        .transaction(move |transaction| Box::pin(deployment_in_transaction(transaction, request)))
        .await
        .map_err(transaction_error)
}

pub(super) async fn replay(
    executor: &PostgresExecutor,
    idempotency: &IdempotencyRequest,
) -> Result<Option<DeploymentBundle>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                Ok(
                    idempotency_replay::<DeploymentBundle>(transaction, &idempotency)
                        .await?
                        .map(|replay| replay.value),
                )
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn deployment_in_transaction(
    transaction: &PostgresTransaction,
    request: CreateDeploymentBundle,
) -> Result<DeploymentBundle, PostgresPersistenceError> {
    if let Some(replay) =
        idempotency_replay::<DeploymentBundle>(transaction, &request.idempotency).await?
    {
        let mut response = replay.value;
        response.replayed = true;
        return Ok(response);
    }
    validate(&request)?;
    let workload = lock_or_insert_workload(transaction, &request.workload).await?;
    require_no_nonterminal_deployment(transaction, &workload).await?;
    require_next_generation(transaction, &request).await?;
    insert_revision(transaction, &request).await?;
    insert_operation(transaction, &request).await?;
    insert_deployment(transaction, &request.deployment).await?;
    replicas::record_generation(
        transaction,
        &workload,
        &request.control,
        &request.revision,
        &request.deployment,
    )
    .await?;

    let response = DeploymentBundle {
        workload,
        revision: request.revision,
        deployment: request.deployment,
        operation: request.operation,
        replayed: false,
    };
    store_outbox(transaction, &request.event).await?;
    store_idempotency(transaction, &request.idempotency, &response).await?;
    Ok(response)
}

async fn require_no_nonterminal_deployment(
    transaction: &PostgresTransaction,
    workload: &Workload,
) -> Result<(), PostgresPersistenceError> {
    // The workload row is already locked by `lock_or_insert_workload`, so all
    // deployment creation for this workload is serialized without a second
    // row-locking statement.
    let existing = fetch_optional::<uuid::Uuid, _>(
        transaction,
        select_from::<Deployments>()
            .select(Deployments::id())
            .filter(Deployments::workload_id().eq(workload.id.as_uuid()))
            .filter(Deployments::status().ne("active"))
            .filter(Deployments::status().ne("failed"))
            .filter(Deployments::status().ne("orphaned"))
            .filter(Deployments::status().ne("cancelled"))
            .limit(1),
    )
    .await?;
    if existing.is_some() {
        return Err(RepositoryError::Conflict(
            "workload already has a nonterminal deployment".into(),
        )
        .into());
    }
    Ok(())
}

fn validate(request: &CreateDeploymentBundle) -> Result<(), PostgresPersistenceError> {
    request
        .control
        .validate()
        .map_err(RepositoryError::Conflict)?;
    request
        .revision
        .validate_agent_binding_for_workload(&request.workload)
        .map_err(RepositoryError::Conflict)?;
    request
        .revision
        .validate_mcp_binding_for_workload(&request.workload)
        .map_err(RepositoryError::Conflict)?;
    request
        .revision
        .validate_skill_bindings_for_workload(&request.workload)
        .map_err(RepositoryError::Conflict)?;
    let workload = &request.workload;
    let revision = &request.revision;
    let deployment = &request.deployment;
    let operation = &request.operation;
    let event = &request.event;
    if revision.workload_id != workload.id
        || request.control.placement_policy.topology() != PlacementTopology::SingleNode
        || deployment.organization_id != workload.organization_id
        || deployment.workload_id != workload.id
        || deployment.revision_id != revision.id
        || deployment.operation_id != operation.id
        || operation.organization_id != workload.organization_id
        || operation.subject.kind() != "deployment"
        || operation.subject.id() != deployment.id.as_uuid()
        || operation.requested_at != deployment.requested_at
        || deployment.status != DeploymentStatus::Queued
        || deployment.node_id.is_some()
        || deployment.command_id.is_some()
        || deployment.cleanup_command_id.is_some()
        || deployment.retirement_command_id.is_some()
        || deployment.failure.is_some()
        || deployment.activated_at.is_some()
        || deployment.cancellation_requested_at.is_some()
        || deployment.cancelled_at.is_some()
        || deployment.aggregate_version != 1
        || event.organization_id() != Some(workload.organization_id.as_uuid())
        || event.aggregate_id != deployment.id.as_uuid()
        || event.aggregate_version != deployment.aggregate_version
        || revision.external_build.as_ref().is_some_and(|external| {
            revision.template.is_none()
                || external.organization_id != workload.organization_id
                || external.project_id != workload.project_id
                || external.environment_id != workload.environment_id
        })
    {
        return Err(RepositoryError::Conflict(
            "deployment creation bundle has inconsistent identities or state".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_or_insert_workload(
    transaction: &PostgresTransaction,
    workload: &Workload,
) -> Result<Workload, PostgresPersistenceError> {
    if let Some(existing) =
        queries::workload_in_transaction(transaction, workload.organization_id, workload.id, true)
            .await?
    {
        if &existing != workload {
            return Err(RepositoryError::Conflict(
                "workload changed before a new revision was requested".into(),
            )
            .into());
        }
        return Ok(existing);
    }

    let inserted = execute(
        transaction,
        insert_into::<Workloads>()
            .value(Workloads::id(), workload.id.as_uuid())
            .value(
                Workloads::organization_id(),
                workload.organization_id.as_uuid(),
            )
            .value(Workloads::project_id(), workload.project_id.as_uuid())
            .value(
                Workloads::environment_id(),
                workload.environment_id.as_uuid(),
            )
            .value(Workloads::name(), workload.name.as_str())
            .value(Workloads::name_key(), workload.name.key())
            .value(Workloads::desired_state(), workload.desired_state.as_str())
            .value(
                Workloads::active_revision_id(),
                workload.active_revision_id.map(|id| id.as_uuid()),
            )
            .value(Workloads::aggregate_version(), workload.aggregate_version)
            .value(Workloads::created_at(), workload.created_at)
            .value(Workloads::updated_at(), workload.updated_at),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("workload", rows)?,
        Err(error) if is_foreign_key_violation(&error) => {
            return Err(RepositoryError::NotFound.into())
        }
        Err(error) if is_unique_violation(&error) => {
            return Err(RepositoryError::Conflict(
                "workload name or identity is already in use".into(),
            )
            .into())
        }
        Err(error) => return Err(error),
    }
    Ok(workload.clone())
}

async fn require_next_generation(
    transaction: &PostgresTransaction,
    request: &CreateDeploymentBundle,
) -> Result<(), PostgresPersistenceError> {
    let next = queries::next_revision_generation(transaction, request.workload.id).await?;
    if request.revision.generation != next {
        return Err(RepositoryError::Conflict(format!(
            "workload revision generation must be {next}"
        ))
        .into());
    }
    Ok(())
}

async fn insert_revision(
    transaction: &PostgresTransaction,
    request: &CreateDeploymentBundle,
) -> Result<(), PostgresPersistenceError> {
    let revision = &request.revision;
    let artifact = revision
        .template
        .as_ref()
        .map(|template| &template.artifact);
    let template = revision
        .template
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;
    let external_build = revision.external_build.as_ref();
    let agent_binding = revision.agent_binding();
    let mcp_binding = revision.mcp_binding();
    let result = execute(
        transaction,
        insert_into::<WorkloadRevisions>()
            .value(WorkloadRevisions::id(), revision.id.as_uuid())
            .value(
                WorkloadRevisions::workload_id(),
                revision.workload_id.as_uuid(),
            )
            .value(WorkloadRevisions::generation(), revision.generation)
            .value(
                WorkloadRevisions::resolution_state(),
                if revision.template.is_some() {
                    "resolved"
                } else {
                    "pending"
                },
            )
            .value(
                WorkloadRevisions::artifact_source_uri(),
                revision.request.artifact.uri.as_str(),
            )
            .value(
                WorkloadRevisions::expected_artifact_digest(),
                revision.request.artifact.expected_digest.clone(),
            )
            .value(
                WorkloadRevisions::template_request(),
                serde_json::to_value(&revision.request)?,
            )
            .value(
                WorkloadRevisions::request_digest(),
                revision.request_digest.as_str(),
            )
            .value(
                WorkloadRevisions::artifact_uri(),
                artifact.map(|artifact| artifact.uri.clone()),
            )
            .value(
                WorkloadRevisions::artifact_digest(),
                artifact.map(|artifact| artifact.digest.clone()),
            )
            .value(
                WorkloadRevisions::artifact_media_type(),
                artifact.map(|artifact| artifact.media_type.clone()),
            )
            .value(WorkloadRevisions::template(), template)
            .value(
                WorkloadRevisions::template_digest(),
                revision.template_digest.clone(),
            )
            .value(WorkloadRevisions::created_at(), revision.created_at)
            .value(WorkloadRevisions::resolved_at(), revision.resolved_at)
            .value(
                WorkloadRevisions::external_build_organization_id(),
                external_build.map(|reference| reference.organization_id.as_uuid()),
            )
            .value(
                WorkloadRevisions::external_build_project_id(),
                external_build.map(|reference| reference.project_id.as_uuid()),
            )
            .value(
                WorkloadRevisions::external_build_environment_id(),
                external_build.map(|reference| reference.environment_id.as_uuid()),
            )
            .value(
                WorkloadRevisions::external_source_revision_id(),
                external_build.map(|reference| reference.source_revision_id.as_uuid()),
            )
            .value(
                WorkloadRevisions::external_build_run_id(),
                external_build.map(|reference| reference.build_run_id.as_uuid()),
            )
            .value(
                WorkloadRevisions::agent_organization_id(),
                agent_binding.map(|binding| binding.organization_id().as_uuid()),
            )
            .value(
                WorkloadRevisions::agent_asset_id(),
                agent_binding.map(|binding| binding.asset_id().as_uuid()),
            )
            .value(
                WorkloadRevisions::agent_asset_release_id(),
                agent_binding.map(|binding| binding.asset_release_id().as_uuid()),
            )
            .value(
                WorkloadRevisions::agent_build_run_id(),
                agent_binding.map(|binding| binding.build_run_id().as_uuid()),
            )
            .value(
                WorkloadRevisions::mcp_organization_id(),
                mcp_binding.map(|binding| binding.organization_id().as_uuid()),
            )
            .value(
                WorkloadRevisions::mcp_asset_id(),
                mcp_binding.map(|binding| binding.asset_id().as_uuid()),
            )
            .value(
                WorkloadRevisions::mcp_asset_release_id(),
                mcp_binding.map(|binding| binding.asset_release_id().as_uuid()),
            )
            .value(
                WorkloadRevisions::mcp_profile_digest(),
                mcp_binding.map(|binding| binding.profile_digest().as_str().to_owned()),
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("workload revision", rows)?,
        Err(error) if is_unique_violation(&error) => {
            return Err(RepositoryError::Conflict(
                "workload revision identity or generation is already in use".into(),
            )
            .into())
        }
        Err(error) => return Err(error),
    }
    for binding in revision.skill_bindings() {
        require_one_row(
            "workload revision Skill binding",
            execute(
                transaction,
                insert_into::<WorkloadRevisionSkillBindings>()
                    .value(
                        WorkloadRevisionSkillBindings::organization_id(),
                        binding.organization_id().as_uuid(),
                    )
                    .value(
                        WorkloadRevisionSkillBindings::workload_id(),
                        revision.workload_id.as_uuid(),
                    )
                    .value(
                        WorkloadRevisionSkillBindings::revision_id(),
                        revision.id.as_uuid(),
                    )
                    .value(
                        WorkloadRevisionSkillBindings::asset_id(),
                        binding.asset_id().as_uuid(),
                    )
                    .value(
                        WorkloadRevisionSkillBindings::asset_release_id(),
                        binding.asset_release_id().as_uuid(),
                    )
                    .value(
                        WorkloadRevisionSkillBindings::artifact_digest(),
                        binding.artifact_digest().as_str(),
                    )
                    .value(
                        WorkloadRevisionSkillBindings::artifact_size_bytes(),
                        binding.artifact_size_bytes(),
                    ),
            )
            .await?,
        )?;
    }
    Ok(())
}

async fn insert_operation(
    transaction: &PostgresTransaction,
    request: &CreateDeploymentBundle,
) -> Result<(), PostgresPersistenceError> {
    operation_requests::insert(transaction, &request.operation).await
}

pub(super) async fn insert_deployment(
    transaction: &PostgresTransaction,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<Deployments>()
            .value(Deployments::id(), deployment.id.as_uuid())
            .value(
                Deployments::organization_id(),
                deployment.organization_id.as_uuid(),
            )
            .value(Deployments::workload_id(), deployment.workload_id.as_uuid())
            .value(Deployments::revision_id(), deployment.revision_id.as_uuid())
            .value(
                Deployments::operation_id(),
                deployment.operation_id.as_uuid(),
            )
            .value(
                Deployments::node_id(),
                deployment.node_id.map(|id| id.as_uuid()),
            )
            .value(
                Deployments::command_id(),
                deployment.command_id.map(|id| id.as_uuid()),
            )
            .value(
                Deployments::cleanup_command_id(),
                deployment.cleanup_command_id.map(|id| id.as_uuid()),
            )
            .value(
                Deployments::retirement_command_id(),
                deployment.retirement_command_id.map(|id| id.as_uuid()),
            )
            .value(Deployments::status(), deployment.status.as_str())
            .value(Deployments::failure(), deployment.failure.clone())
            .value(
                Deployments::aggregate_version(),
                deployment.aggregate_version,
            )
            .value(Deployments::requested_at(), deployment.requested_at)
            .value(Deployments::updated_at(), deployment.updated_at)
            .value(Deployments::activated_at(), deployment.activated_at)
            .value(
                Deployments::cancellation_requested_at(),
                deployment.cancellation_requested_at,
            )
            .value(Deployments::cancelled_at(), deployment.cancelled_at),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("deployment", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "deployment identity or revision is already in use".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}
