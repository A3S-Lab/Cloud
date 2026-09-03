use crate::modules::durable_cells::application::{
    DurableCellExecution, DurableCellExecutionArtifactMount, DurableCellExecutionAuthority,
    DurableCellExecutionCancellationRequest, DurableCellExecutionRequest,
    DurableCellExecutionStatus, DurableCellExecutionTaskPolicy, DurableCellExecutionTemplate,
    IDurableCellExecutionPort,
};
use crate::modules::executions::application::{
    validate_bound_execution, BoundExecutionCreation, ExecutionCancellation,
    ExecutionCancellationService, ExecutionCreator,
};
use crate::modules::executions::domain::{
    Execution, ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionStatus,
    ExecutionTaskArtifactMount, ExecutionTaskAuthority, ExecutionTaskPolicy, ExecutionTaskSecret,
    ExecutionTaskSecretTarget, ExecutionTemplate, IExecutionRepository,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{ExecutionId, OrganizationId, Sha256Digest};
use a3s_cloud_contracts::CloudSecretReference;
use a3s_runtime::contract::SecretTarget;
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from the existing Executions owner into the
/// Durable Cells consumer-owned finite-Task port. Executions retains the
/// aggregate, idempotency, event, Operation, Flow, and Runtime lifecycle.
#[derive(Clone)]
pub(crate) struct ExecutionsDurableCellExecutionAdapter {
    executions: Arc<dyn IExecutionRepository>,
    creator: ExecutionCreator,
    cancellations: ExecutionCancellationService,
}

impl ExecutionsDurableCellExecutionAdapter {
    pub(crate) fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        executions: Arc<dyn IExecutionRepository>,
    ) -> Self {
        Self {
            creator: ExecutionCreator::new(environments, Arc::clone(&executions)),
            cancellations: ExecutionCancellationService::new(Arc::clone(&executions)),
            executions,
        }
    }
}

#[async_trait]
impl IDurableCellExecutionPort for ExecutionsDurableCellExecutionAdapter {
    async fn find_bound_task(
        &self,
        organization_id: OrganizationId,
        execution_id: ExecutionId,
    ) -> ApplicationResult<Option<DurableCellExecution>> {
        let execution = self.executions.find(organization_id, execution_id).await?;
        execution.map(project_execution).transpose()
    }

    async fn ensure_bound_task(
        &self,
        request: &DurableCellExecutionRequest,
    ) -> ApplicationResult<DurableCellExecution> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let creation = owner_creation(request).map_err(ApplicationError::Invalid)?;
        if let Some(execution) = self
            .executions
            .find(request.organization_id, request.execution_id)
            .await?
        {
            let execution = restore_bound_execution(execution)?;
            validate_bound_execution(&creation, &execution)?;
            return project_execution(execution);
        }
        let result = self.creator.create_bound_task(creation).await?;
        project_execution(result.execution)
    }

    async fn cancel_bound_task(
        &self,
        request: &DurableCellExecutionCancellationRequest,
    ) -> ApplicationResult<DurableCellExecution> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let execution = self
            .executions
            .find(request.organization_id, request.execution_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound("Durable Cell Execution not found".into()))?;
        let execution = restore_bound_execution(execution)?;
        validate_cancellation_target(request, &execution)?;
        if !execution.status.is_terminal()
            && !matches!(
                execution.status,
                ExecutionStatus::Cancelling | ExecutionStatus::CleanupPending
            )
        {
            let result = self
                .cancellations
                .cancel(ExecutionCancellation {
                    execution,
                    idempotency_key: request.idempotency_key.clone(),
                    request_id: request.request_id,
                    requested_at: request.requested_at,
                })
                .await?;
            return project_execution(result.execution);
        }
        project_execution(execution)
    }
}

fn validate_cancellation_target(
    request: &DurableCellExecutionCancellationRequest,
    execution: &Execution,
) -> ApplicationResult<()> {
    let authority = execution
        .task_policy
        .as_ref()
        .map(ExecutionTaskPolicy::authority);
    if execution.organization_id != request.organization_id
        || execution.project_id != request.project_id
        || execution.environment_id != request.environment_id
        || execution.id != request.execution_id
        || authority.is_none_or(|authority| {
            authority.kind() != request.authority_kind
                || authority.subject_id() != request.authority_subject_id
        })
    {
        return Err(ApplicationError::Conflict(
            "Durable Cell cancellation target changed its immutable identity".into(),
        ));
    }
    Ok(())
}

fn owner_creation(request: &DurableCellExecutionRequest) -> Result<BoundExecutionCreation, String> {
    Ok(BoundExecutionCreation {
        organization_id: request.organization_id,
        project_id: request.project_id,
        environment_id: request.environment_id,
        execution_id: request.execution_id,
        template: owner_template(&request.template)?,
        target_node_id: request.target_node_id,
        task_policy: owner_task_policy(&request.task_policy)?,
        idempotency_key: request.idempotency_key.clone(),
        request_id: request.request_id,
        requested_at: request.requested_at,
    })
}

fn owner_template(template: &DurableCellExecutionTemplate) -> Result<ExecutionTemplate, String> {
    let timeout_ms = template
        .resources
        .execution_timeout_ms
        .ok_or_else(|| "Durable Cell finite Task requires an execution timeout".to_owned())?;
    Ok(ExecutionTemplate {
        artifact: ExecutionArtifact {
            uri: template.artifact.uri.clone(),
            digest: template.artifact.digest.clone(),
            media_type: template.artifact.media_type.clone(),
        },
        process: ExecutionProcess {
            command: template.process.command.clone(),
            args: template.process.args.clone(),
            working_directory: template.process.working_directory.clone(),
            environment: template.process.environment.clone(),
        },
        input: template.input.clone(),
        resources: ExecutionResources {
            cpu_millis: template.resources.cpu_millis,
            memory_bytes: template.resources.memory_bytes,
            pids: template.resources.pids,
            ephemeral_storage_bytes: template.resources.ephemeral_storage_bytes,
            timeout_ms,
        },
    })
}

fn owner_task_policy(
    policy: &DurableCellExecutionTaskPolicy,
) -> Result<ExecutionTaskPolicy, String> {
    let authority = owner_authority(&policy.authority)?;
    let mounts = policy
        .mounts
        .iter()
        .map(owner_mount)
        .collect::<Result<Vec<_>, _>>()?;
    let secrets = policy
        .secrets
        .iter()
        .map(|secret| {
            let target = match &secret.target {
                SecretTarget::Environment { variable } => ExecutionTaskSecretTarget::Environment {
                    variable: variable.clone(),
                },
                SecretTarget::File { path, mode } => ExecutionTaskSecretTarget::File {
                    path: path.clone(),
                    mode: *mode,
                },
                SecretTarget::RegistryCredential => ExecutionTaskSecretTarget::RegistryCredential,
            };
            ExecutionTaskSecret::new(
                secret.name.clone(),
                CloudSecretReference::parse(&secret.reference)?,
                target,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    ExecutionTaskPolicy::new(
        authority,
        mounts,
        secrets,
        policy.semantics_profile_digest.clone(),
    )
}

fn owner_authority(
    authority: &DurableCellExecutionAuthority,
) -> Result<ExecutionTaskAuthority, String> {
    ExecutionTaskAuthority::new(
        authority.kind.clone(),
        authority.subject_id,
        authority.digest.clone(),
    )
}

fn owner_mount(
    mount: &DurableCellExecutionArtifactMount,
) -> Result<ExecutionTaskArtifactMount, String> {
    ExecutionTaskArtifactMount::new(
        mount.name.clone(),
        mount.artifact.uri.clone(),
        Sha256Digest::parse(mount.artifact.digest.clone())?,
        mount.artifact.media_type.clone(),
        mount.target.clone(),
    )
}

fn restore_bound_execution(execution: Execution) -> ApplicationResult<Execution> {
    let execution = execution.restore().map_err(|error| {
        ApplicationError::Internal(format!(
            "stored Durable Cell bound Execution failed integrity validation: {error}"
        ))
    })?;
    if execution.workflow.is_some()
        || execution.target_node_id.is_none()
        || execution.task_policy.is_none()
    {
        return Err(ApplicationError::Conflict(
            "Durable Cell publication requires an internal node-bound Execution Task".into(),
        ));
    }
    Ok(execution)
}

fn project_execution(execution: Execution) -> ApplicationResult<DurableCellExecution> {
    let execution = restore_bound_execution(execution)?;
    let target_node_id = execution.target_node_id.ok_or_else(|| {
        ApplicationError::Internal(
            "Durable Cell bound Execution omitted its target Node after validation".into(),
        )
    })?;
    let task_policy = execution.task_policy.as_ref().ok_or_else(|| {
        ApplicationError::Internal(
            "Durable Cell bound Execution omitted its Task policy after validation".into(),
        )
    })?;
    let authority = task_policy.authority();
    let status = match execution.status {
        ExecutionStatus::Queued => DurableCellExecutionStatus::Queued,
        ExecutionStatus::Scheduled => DurableCellExecutionStatus::Scheduled,
        ExecutionStatus::Running => DurableCellExecutionStatus::Running,
        ExecutionStatus::Cancelling => DurableCellExecutionStatus::Cancelling,
        ExecutionStatus::CleanupPending => DurableCellExecutionStatus::CleanupPending,
        ExecutionStatus::Succeeded => DurableCellExecutionStatus::Succeeded,
        ExecutionStatus::Failed => DurableCellExecutionStatus::Failed,
        ExecutionStatus::Cancelled => DurableCellExecutionStatus::Cancelled,
    };
    let projection = DurableCellExecution {
        organization_id: execution.organization_id,
        project_id: execution.project_id,
        environment_id: execution.environment_id,
        id: execution.id,
        target_node_id,
        authority_kind: authority.kind().to_owned(),
        authority_subject_id: authority.subject_id(),
        authority_digest: authority.digest().clone(),
        status,
        aggregate_version: execution.aggregate_version,
        finished_at: execution.finished_at,
    };
    projection.validate().map_err(|error| {
        ApplicationError::Internal(format!("invalid Execution projection: {error}"))
    })?;
    Ok(projection)
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn materialize_bound_execution_for_conformance(
    request: &DurableCellExecutionRequest,
) -> Result<Execution, String> {
    request.validate()?;
    let creation = owner_creation(request)?;
    Execution::create_bound_task(
        creation.organization_id,
        creation.project_id,
        creation.environment_id,
        creation.execution_id,
        creation.template,
        creation.target_node_id,
        creation.task_policy,
        creation.requested_at,
    )
}
