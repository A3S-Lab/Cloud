use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeCommandId, NodeId, OperationId, OrganizationId, ProjectId,
    RepositoryError, ResourceName, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, RequestedServiceTemplate, ServiceTemplate, Workload,
    WorkloadDesiredState, WorkloadRevision,
};
use a3s_orm::{DecodeError, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

pub(super) const SELECT_WORKLOADS: &str = "select id, organization_id, project_id, environment_id, name, desired_state, active_revision_id, aggregate_version, created_at, updated_at from workloads";
pub(super) const SELECT_REVISIONS: &str = "select r.id, r.workload_id, r.generation, r.resolution_state, r.artifact_source_uri, r.expected_artifact_digest, r.template_request, r.request_digest, r.artifact_uri, r.artifact_digest, r.artifact_media_type, r.template, r.template_digest, r.created_at, r.resolved_at from workload_revisions r";
pub(super) const SELECT_DEPLOYMENTS: &str = "select id, organization_id, workload_id, revision_id, operation_id, node_id, command_id, cleanup_command_id, retirement_command_id, status, failure, aggregate_version, requested_at, updated_at, activated_at, cancellation_requested_at, cancelled_at from deployments";

pub(super) struct WorkloadRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    name: String,
    desired_state: String,
    active_revision_id: Option<Uuid>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

pub(super) struct RevisionRow {
    id: Uuid,
    workload_id: Uuid,
    generation: u64,
    resolution_state: String,
    artifact_source_uri: String,
    expected_artifact_digest: Option<String>,
    template_request: Value,
    request_digest: String,
    artifact_uri: Option<String>,
    artifact_digest: Option<String>,
    artifact_media_type: Option<String>,
    template: Option<Value>,
    template_digest: Option<String>,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

pub(super) struct DeploymentRow {
    id: Uuid,
    organization_id: Uuid,
    workload_id: Uuid,
    revision_id: Uuid,
    operation_id: Uuid,
    node_id: Option<Uuid>,
    command_id: Option<Uuid>,
    cleanup_command_id: Option<Uuid>,
    retirement_command_id: Option<Uuid>,
    status: String,
    failure: Option<String>,
    aggregate_version: u64,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    activated_at: Option<DateTime<Utc>>,
    cancellation_requested_at: Option<DateTime<Utc>>,
    cancelled_at: Option<DateTime<Utc>>,
}

macro_rules! from_row {
    ($row:ty, { $($field:ident: $index:literal),+ $(,)? }) => {
        impl FromRow for $row {
            fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
                Ok(Self { $($field: decode(row, $index)?,)+ })
            }
        }
    };
}

from_row!(WorkloadRow, {
    id: 0, organization_id: 1, project_id: 2, environment_id: 3, name: 4,
    desired_state: 5, active_revision_id: 6, aggregate_version: 7, created_at: 8,
    updated_at: 9,
});
from_row!(RevisionRow, {
    id: 0, workload_id: 1, generation: 2, resolution_state: 3,
    artifact_source_uri: 4, expected_artifact_digest: 5, template_request: 6,
    request_digest: 7, artifact_uri: 8, artifact_digest: 9,
    artifact_media_type: 10, template: 11, template_digest: 12, created_at: 13,
    resolved_at: 14,
});
from_row!(DeploymentRow, {
    id: 0, organization_id: 1, workload_id: 2, revision_id: 3, operation_id: 4,
    node_id: 5, command_id: 6, cleanup_command_id: 7, retirement_command_id: 8,
    status: 9, failure: 10, aggregate_version: 11, requested_at: 12, updated_at: 13,
    activated_at: 14, cancellation_requested_at: 15, cancelled_at: 16,
});
fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

pub(super) fn workload(row: WorkloadRow) -> Result<Workload, RepositoryError> {
    if row.aggregate_version == 0 || row.updated_at < row.created_at {
        return Err(corrupt("workload version or timestamps are invalid"));
    }
    Ok(Workload {
        id: WorkloadId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        name: ResourceName::parse(row.name)
            .map_err(|error| corrupt(format!("workload name is invalid: {error}")))?,
        desired_state: WorkloadDesiredState::parse(&row.desired_state)
            .map_err(|error| corrupt(format!("workload desired state is invalid: {error}")))?,
        active_revision_id: row.active_revision_id.map(WorkloadRevisionId::from_uuid),
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub(super) fn revision(row: RevisionRow) -> Result<WorkloadRevision, RepositoryError> {
    let request: RequestedServiceTemplate = serde_json::from_value(row.template_request)
        .map_err(|error| corrupt(format!("workload template request is invalid: {error}")))?;
    if request.artifact.uri != row.artifact_source_uri
        || request.artifact.expected_digest != row.expected_artifact_digest
    {
        return Err(corrupt(
            "workload revision source columns do not match its template request",
        ));
    }
    let mut revision = WorkloadRevision::request(
        WorkloadRevisionId::from_uuid(row.id),
        WorkloadId::from_uuid(row.workload_id),
        row.generation,
        request,
        row.created_at,
    )
    .map_err(|error| corrupt(format!("workload revision is invalid: {error}")))?;
    if revision.request_digest != row.request_digest {
        return Err(corrupt(
            "workload revision request digest does not match its template request",
        ));
    }
    match row.resolution_state.as_str() {
        "pending"
            if row.artifact_uri.is_none()
                && row.artifact_digest.is_none()
                && row.artifact_media_type.is_none()
                && row.template.is_none()
                && row.template_digest.is_none()
                && row.resolved_at.is_none() =>
        {
            Ok(revision)
        }
        "resolved" => {
            let template: ServiceTemplate = serde_json::from_value(
                row.template
                    .ok_or_else(|| corrupt("resolved workload revision omitted its template"))?,
            )
            .map_err(|error| corrupt(format!("workload template is invalid: {error}")))?;
            if Some(template.artifact.uri.as_str()) != row.artifact_uri.as_deref()
                || Some(template.artifact.digest.as_str()) != row.artifact_digest.as_deref()
                || Some(template.artifact.media_type.as_str()) != row.artifact_media_type.as_deref()
            {
                return Err(corrupt(
                    "workload revision artifact columns do not match its template",
                ));
            }
            let resolved_at = row
                .resolved_at
                .ok_or_else(|| corrupt("resolved workload revision omitted its resolution time"))?;
            revision
                .resolve(template.artifact.clone(), resolved_at)
                .map_err(|error| {
                    corrupt(format!("workload revision resolution is invalid: {error}"))
                })?;
            if revision.template.as_ref() != Some(&template)
                || revision.template_digest != row.template_digest
            {
                return Err(corrupt(
                    "workload revision template digest does not match its template",
                ));
            }
            Ok(revision)
        }
        _ => Err(corrupt(
            "workload revision resolution state does not match its resolved fields",
        )),
    }
}

pub(super) fn deployment(row: DeploymentRow) -> Result<Deployment, RepositoryError> {
    let status = DeploymentStatus::parse(&row.status)
        .map_err(|error| corrupt(format!("deployment status is invalid: {error}")))?;
    let node_id = row.node_id.map(NodeId::from_uuid);
    let command_id = row.command_id.map(NodeCommandId::from_uuid);
    let cleanup_command_id = row.cleanup_command_id.map(NodeCommandId::from_uuid);
    let retirement_command_id = row.retirement_command_id.map(NodeCommandId::from_uuid);
    let state_is_valid = match status {
        DeploymentStatus::Queued | DeploymentStatus::Resolving => {
            node_id.is_none()
                && command_id.is_none()
                && cleanup_command_id.is_none()
                && retirement_command_id.is_none()
        }
        DeploymentStatus::Scheduled => {
            node_id.is_some()
                && command_id.is_none()
                && cleanup_command_id.is_none()
                && retirement_command_id.is_none()
        }
        DeploymentStatus::Applying | DeploymentStatus::Verifying => {
            node_id.is_some()
                && command_id.is_some()
                && cleanup_command_id.is_none()
                && retirement_command_id.is_none()
        }
        DeploymentStatus::Retiring | DeploymentStatus::Active => {
            node_id.is_some() && command_id.is_some() && cleanup_command_id.is_none()
        }
        DeploymentStatus::Cancelling => {
            cleanup_command_id.is_none() && retirement_command_id.is_none()
        }
        DeploymentStatus::CleanupPending => {
            node_id.is_some()
                && command_id.is_some()
                && cleanup_command_id.is_some()
                && retirement_command_id.is_none()
        }
        DeploymentStatus::Failed | DeploymentStatus::Orphaned | DeploymentStatus::Cancelled => {
            command_id.is_none() || node_id.is_some()
        }
    };
    if row.aggregate_version == 0
        || row.updated_at < row.requested_at
        || !state_is_valid
        || matches!(
            status,
            DeploymentStatus::Failed | DeploymentStatus::Orphaned
        ) != row.failure.is_some()
        || match status {
            DeploymentStatus::Retiring | DeploymentStatus::Active => row.activated_at.is_none(),
            DeploymentStatus::Orphaned => false,
            _ => row.activated_at.is_some(),
        }
        || matches!(
            status,
            DeploymentStatus::Cancelling
                | DeploymentStatus::CleanupPending
                | DeploymentStatus::Cancelled
        ) != row.cancellation_requested_at.is_some()
            && status != DeploymentStatus::Orphaned
        || (status == DeploymentStatus::Cancelled) != row.cancelled_at.is_some()
    {
        return Err(corrupt("deployment row violates its state invariants"));
    }
    Ok(Deployment {
        id: DeploymentId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        workload_id: WorkloadId::from_uuid(row.workload_id),
        revision_id: WorkloadRevisionId::from_uuid(row.revision_id),
        operation_id: OperationId::from_uuid(row.operation_id),
        node_id,
        command_id,
        cleanup_command_id,
        retirement_command_id,
        status,
        failure: row.failure,
        aggregate_version: row.aggregate_version,
        requested_at: row.requested_at,
        updated_at: row.updated_at,
        activated_at: row.activated_at,
        cancellation_requested_at: row.cancellation_requested_at,
        cancelled_at: row.cancelled_at,
    })
}

fn corrupt(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(format!("stored data is corrupt: {}", message.into()))
}
