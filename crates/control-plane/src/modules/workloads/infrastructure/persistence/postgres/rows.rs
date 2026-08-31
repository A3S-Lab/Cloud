use crate::modules::assets::domain::McpServiceProfile;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, DeploymentId, EnvironmentId, NodeCommandId, NodeId,
    OperationId, OrganizationId, ProjectId, RepositoryError, ResourceName, Sha256Digest,
    SourceRevisionId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    AgentReleaseRuntimeContract, AgentWorkloadRevisionBinding, Deployment, DeploymentStatus,
    ExternalBuildReference, McpWorkloadRevisionBinding, RequestedServiceTemplate, ServiceTemplate,
    Workload, WorkloadDesiredState, WorkloadRevision,
};
use a3s_orm::expression::Selection;
use a3s_orm::{DecodeError, Expression, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::schema::{Deployments, McpServiceProfiles, WorkloadRevisions, Workloads};

pub(super) struct WorkloadSelection;
pub(super) struct RevisionSelection;
pub(super) struct DeploymentSelection;

impl Selection for WorkloadSelection {
    type Output = WorkloadRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            Workloads::id().expression(),
            Workloads::organization_id().expression(),
            Workloads::project_id().expression(),
            Workloads::environment_id().expression(),
            Workloads::name().expression(),
            Workloads::desired_state().expression(),
            Workloads::active_revision_id().expression(),
            Workloads::aggregate_version().expression(),
            Workloads::created_at().expression(),
            Workloads::updated_at().expression(),
        ]
    }
}

impl Selection for RevisionSelection {
    type Output = RevisionRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkloadRevisions::id().expression(),
            WorkloadRevisions::workload_id().expression(),
            WorkloadRevisions::generation().expression(),
            WorkloadRevisions::resolution_state().expression(),
            WorkloadRevisions::artifact_source_uri().expression(),
            WorkloadRevisions::expected_artifact_digest().expression(),
            WorkloadRevisions::template_request().expression(),
            WorkloadRevisions::request_digest().expression(),
            WorkloadRevisions::artifact_uri().expression(),
            WorkloadRevisions::artifact_digest().expression(),
            WorkloadRevisions::artifact_media_type().expression(),
            WorkloadRevisions::template().expression(),
            WorkloadRevisions::template_digest().expression(),
            WorkloadRevisions::created_at().expression(),
            WorkloadRevisions::resolved_at().expression(),
            WorkloadRevisions::external_build_organization_id().expression(),
            WorkloadRevisions::external_build_project_id().expression(),
            WorkloadRevisions::external_build_environment_id().expression(),
            WorkloadRevisions::external_source_revision_id().expression(),
            WorkloadRevisions::external_build_run_id().expression(),
            WorkloadRevisions::agent_organization_id().expression(),
            WorkloadRevisions::agent_asset_id().expression(),
            WorkloadRevisions::agent_asset_release_id().expression(),
            WorkloadRevisions::agent_build_run_id().expression(),
            WorkloadRevisions::agent_release_contract().expression(),
            WorkloadRevisions::mcp_organization_id().expression(),
            WorkloadRevisions::mcp_asset_id().expression(),
            WorkloadRevisions::mcp_asset_release_id().expression(),
            WorkloadRevisions::mcp_profile_digest().expression(),
            McpServiceProfiles::acl().expression(),
        ]
    }
}

impl Selection for DeploymentSelection {
    type Output = DeploymentRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            Deployments::id().expression(),
            Deployments::organization_id().expression(),
            Deployments::workload_id().expression(),
            Deployments::revision_id().expression(),
            Deployments::operation_id().expression(),
            Deployments::node_id().expression(),
            Deployments::command_id().expression(),
            Deployments::cleanup_command_id().expression(),
            Deployments::retirement_command_id().expression(),
            Deployments::status().expression(),
            Deployments::failure().expression(),
            Deployments::aggregate_version().expression(),
            Deployments::requested_at().expression(),
            Deployments::updated_at().expression(),
            Deployments::activated_at().expression(),
            Deployments::cancellation_requested_at().expression(),
            Deployments::cancelled_at().expression(),
        ]
    }
}

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
    external_build_organization_id: Option<Uuid>,
    external_build_project_id: Option<Uuid>,
    external_build_environment_id: Option<Uuid>,
    external_source_revision_id: Option<Uuid>,
    external_build_run_id: Option<Uuid>,
    agent_organization_id: Option<Uuid>,
    agent_asset_id: Option<Uuid>,
    agent_asset_release_id: Option<Uuid>,
    agent_build_run_id: Option<Uuid>,
    agent_release_contract: Option<Value>,
    mcp_organization_id: Option<Uuid>,
    mcp_asset_id: Option<Uuid>,
    mcp_asset_release_id: Option<Uuid>,
    mcp_profile_digest: Option<String>,
    mcp_profile_acl: Option<String>,
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
    resolved_at: 14, external_build_organization_id: 15,
    external_build_project_id: 16, external_build_environment_id: 17,
    external_source_revision_id: 18, external_build_run_id: 19,
    agent_organization_id: 20, agent_asset_id: 21, agent_asset_release_id: 22,
    agent_build_run_id: 23, agent_release_contract: 24, mcp_organization_id: 25,
    mcp_asset_id: 26, mcp_asset_release_id: 27, mcp_profile_digest: 28,
    mcp_profile_acl: 29,
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
    let external_build = match (
        row.external_build_organization_id,
        row.external_build_project_id,
        row.external_build_environment_id,
        row.external_source_revision_id,
        row.external_build_run_id,
    ) {
        (None, None, None, None, None) => None,
        (
            Some(organization_id),
            Some(project_id),
            Some(environment_id),
            Some(source_revision_id),
            Some(build_run_id),
        ) => Some(ExternalBuildReference {
            organization_id: OrganizationId::from_uuid(organization_id),
            project_id: ProjectId::from_uuid(project_id),
            environment_id: EnvironmentId::from_uuid(environment_id),
            source_revision_id: SourceRevisionId::from_uuid(source_revision_id),
            build_run_id: BuildRunId::from_uuid(build_run_id),
        }),
        _ => {
            return Err(corrupt(
                "workload revision external build reference is incomplete",
            ))
        }
    };
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
                && row.resolved_at.is_none() => {}
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
        }
        _ => {
            return Err(corrupt(
                "workload revision resolution state does not match its resolved fields",
            ))
        }
    }
    if let Some(external_build) = external_build {
        revision
            .restore_external_build(external_build)
            .map_err(|error| {
                corrupt(format!(
                    "workload revision external build reference is invalid: {error}"
                ))
            })?;
    }
    match (
        row.agent_organization_id,
        row.agent_asset_id,
        row.agent_asset_release_id,
        row.agent_build_run_id,
        row.agent_release_contract,
    ) {
        (None, None, None, None, None) => {}
        (
            Some(organization_id),
            Some(asset_id),
            Some(asset_release_id),
            Some(build_run_id),
            runtime_contract,
        ) => {
            let runtime_contract = runtime_contract
                .map(serde_json::from_value::<AgentReleaseRuntimeContract>)
                .transpose()
                .map_err(|error| {
                    corrupt(format!(
                        "Agent release Runtime contract is invalid: {error}"
                    ))
                })?;
            let binding = AgentWorkloadRevisionBinding::restore_with_contract(
                OrganizationId::from_uuid(organization_id),
                AssetId::from_uuid(asset_id),
                AssetReleaseId::from_uuid(asset_release_id),
                BuildRunId::from_uuid(build_run_id),
                runtime_contract,
            )
            .map_err(|error| {
                corrupt(format!(
                    "Agent Workload release binding is invalid: {error}"
                ))
            })?;
            revision.restore_agent_binding(binding).map_err(|error| {
                corrupt(format!(
                    "Agent Workload revision binding is invalid: {error}"
                ))
            })?;
        }
        _ => {
            return Err(corrupt(
                "workload revision Agent release binding is incomplete",
            ))
        }
    }
    match (
        row.mcp_organization_id,
        row.mcp_asset_id,
        row.mcp_asset_release_id,
        row.mcp_profile_digest,
        row.mcp_profile_acl,
    ) {
        (None, None, None, None, None) => {}
        (
            Some(organization_id),
            Some(asset_id),
            Some(asset_release_id),
            Some(profile_digest),
            Some(profile_acl),
        ) => {
            let profile =
                McpServiceProfile::restore(&profile_acl, &profile_digest).map_err(|error| {
                    corrupt(format!("MCP Workload Service profile is invalid: {error}"))
                })?;
            let binding = McpWorkloadRevisionBinding::restore(
                OrganizationId::from_uuid(organization_id),
                AssetId::from_uuid(asset_id),
                AssetReleaseId::from_uuid(asset_release_id),
                Sha256Digest::parse(profile_digest).map_err(|error| {
                    corrupt(format!("MCP Workload profile digest is invalid: {error}"))
                })?,
            )
            .map_err(|error| {
                corrupt(format!("MCP Workload release binding is invalid: {error}"))
            })?;
            revision
                .restore_mcp_binding(binding, &profile)
                .map_err(|error| {
                    corrupt(format!("MCP Workload revision binding is invalid: {error}"))
                })?;
        }
        _ => {
            return Err(corrupt(
                "workload revision MCP release binding or Service profile is incomplete",
            ))
        }
    }
    Ok(revision)
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
