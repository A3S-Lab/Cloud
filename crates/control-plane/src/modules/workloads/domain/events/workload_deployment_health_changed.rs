use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeId, OperationId, OrganizationId, ProjectId, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, Workload, WorkloadDesiredState, WorkloadRevision,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadDeploymentHealthStatus {
    Failed,
    Healthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadDeploymentFailurePhase {
    Queued,
    Resolving,
    Scheduled,
    Applying,
    Verifying,
}

impl WorkloadDeploymentFailurePhase {
    const fn from_status(status: DeploymentStatus) -> Option<Self> {
        match status {
            DeploymentStatus::Queued => Some(Self::Queued),
            DeploymentStatus::Resolving => Some(Self::Resolving),
            DeploymentStatus::Scheduled => Some(Self::Scheduled),
            DeploymentStatus::Applying => Some(Self::Applying),
            DeploymentStatus::Verifying => Some(Self::Verifying),
            DeploymentStatus::Retiring
            | DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Active
            | DeploymentStatus::Failed
            | DeploymentStatus::Orphaned
            | DeploymentStatus::Cancelled => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadDeploymentAvailabilityImpact {
    Unavailable,
    PreviousRevisionRetained,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadDeploymentHealthChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_name: String,
    pub deployment_id: DeploymentId,
    pub revision_id: WorkloadRevisionId,
    pub revision_generation: u64,
    pub operation_id: OperationId,
    pub node_id: Option<NodeId>,
    pub status: WorkloadDeploymentHealthStatus,
    pub failure_phase: Option<WorkloadDeploymentFailurePhase>,
    pub availability_impact: Option<WorkloadDeploymentAvailabilityImpact>,
}

impl WorkloadDeploymentHealthChanged {
    pub fn failure_envelope(
        previous: &Deployment,
        deployment: &Deployment,
        workload: &Workload,
        revision: &WorkloadRevision,
    ) -> Result<Option<DomainEventEnvelope>, String> {
        validate_transition_context(previous, deployment, workload, revision)?;
        let reason = deployment.failure.clone().ok_or_else(|| {
            "Workload deployment failure fact omitted the private failure reason".to_owned()
        })?;
        let mut expected = previous.clone();
        expected.fail(reason, deployment.updated_at)?;
        if expected != *deployment {
            return Err("Workload deployment failure fact has an invalid transition".into());
        }
        let Some(failure_phase) = WorkloadDeploymentFailurePhase::from_status(previous.status)
        else {
            return Ok(None);
        };
        if workload.desired_state != WorkloadDesiredState::Running
            || workload.active_revision_id == Some(deployment.revision_id)
        {
            return Ok(None);
        }
        let availability_impact = match workload.active_revision_id {
            Some(_) => WorkloadDeploymentAvailabilityImpact::PreviousRevisionRetained,
            None => WorkloadDeploymentAvailabilityImpact::Unavailable,
        };
        Ok(Some(envelope(
            "workload.deployment.failed",
            deployment,
            workload,
            revision,
            WorkloadDeploymentHealthStatus::Failed,
            Some(failure_phase),
            Some(availability_impact),
        )?))
    }

    pub fn healthy_envelope(
        previous_deployment: &Deployment,
        deployment: &Deployment,
        previous_workload: &Workload,
        workload: &Workload,
        revision: &WorkloadRevision,
    ) -> Result<Option<DomainEventEnvelope>, String> {
        validate_transition_context(previous_deployment, deployment, workload, revision)?;
        validate_workload_transition(previous_workload, workload)?;
        let activated_at = deployment
            .activated_at
            .ok_or_else(|| "healthy Workload deployment omitted its activation time".to_owned())?;
        let retirement_required = deployment.status == DeploymentStatus::Retiring;
        let mut expected_deployment = previous_deployment.clone();
        expected_deployment.activate(retirement_required, activated_at)?;
        let mut expected_workload = previous_workload.clone();
        expected_workload.activate(deployment.revision_id, activated_at)?;
        if expected_deployment != *deployment || expected_workload != *workload {
            return Err("Workload deployment healthy fact has an invalid transition".into());
        }
        if previous_workload.active_revision_id == Some(deployment.revision_id) {
            return Ok(None);
        }
        Ok(Some(envelope(
            "workload.deployment.healthy",
            deployment,
            workload,
            revision,
            WorkloadDeploymentHealthStatus::Healthy,
            None,
            None,
        )?))
    }
}

fn validate_transition_context(
    previous: &Deployment,
    deployment: &Deployment,
    workload: &Workload,
    revision: &WorkloadRevision,
) -> Result<(), String> {
    if workload.id.as_uuid().is_nil()
        || workload.organization_id.as_uuid().is_nil()
        || workload.project_id.as_uuid().is_nil()
        || workload.environment_id.as_uuid().is_nil()
        || deployment.id.as_uuid().is_nil()
        || deployment.operation_id.as_uuid().is_nil()
        || revision.id.as_uuid().is_nil()
        || revision.generation == 0
        || deployment.organization_id != workload.organization_id
        || deployment.workload_id != workload.id
        || deployment.revision_id != revision.id
        || revision.workload_id != workload.id
        || previous.id != deployment.id
        || previous.organization_id != deployment.organization_id
        || previous.workload_id != deployment.workload_id
        || previous.revision_id != deployment.revision_id
        || previous.operation_id != deployment.operation_id
        || previous.node_id != deployment.node_id
        || previous.command_id != deployment.command_id
        || previous.cleanup_command_id != deployment.cleanup_command_id
        || previous.retirement_command_id != deployment.retirement_command_id
        || previous.requested_at != deployment.requested_at
        || deployment
            .node_id
            .is_some_and(|node_id| node_id.as_uuid().is_nil())
    {
        return Err("Workload deployment health fact identity is inconsistent".into());
    }
    Ok(())
}

fn validate_workload_transition(previous: &Workload, workload: &Workload) -> Result<(), String> {
    if previous.id != workload.id
        || previous.organization_id != workload.organization_id
        || previous.project_id != workload.project_id
        || previous.environment_id != workload.environment_id
        || previous.name != workload.name
        || previous.desired_state != workload.desired_state
        || previous.created_at != workload.created_at
    {
        return Err("Workload deployment health fact owner transition is inconsistent".into());
    }
    Ok(())
}

fn envelope(
    event_key: &str,
    deployment: &Deployment,
    workload: &Workload,
    revision: &WorkloadRevision,
    status: WorkloadDeploymentHealthStatus,
    failure_phase: Option<WorkloadDeploymentFailurePhase>,
    availability_impact: Option<WorkloadDeploymentAvailabilityImpact>,
) -> Result<DomainEventEnvelope, String> {
    let payload = WorkloadDeploymentHealthChanged {
        organization_id: workload.organization_id,
        project_id: workload.project_id,
        environment_id: workload.environment_id,
        workload_id: workload.id,
        workload_name: workload.name.as_str().to_owned(),
        deployment_id: deployment.id,
        revision_id: revision.id,
        revision_generation: revision.generation,
        operation_id: deployment.operation_id,
        node_id: deployment.node_id,
        status,
        failure_phase,
        availability_impact,
    };
    Ok(DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: workload.organization_id.as_uuid(),
        aggregate_id: workload.id.as_uuid(),
        aggregate_version: revision.generation,
        occurred_at: deployment.updated_at,
        correlation_id: deployment.operation_id.as_uuid(),
        causation_id: deployment.command_id.map(|command_id| command_id.as_uuid()),
        payload: serde_json::to_value(payload).map_err(|error| {
            format!("could not encode Workload deployment health fact: {error}")
        })?,
    })
}
