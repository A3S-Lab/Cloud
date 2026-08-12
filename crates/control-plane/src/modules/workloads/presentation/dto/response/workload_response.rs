use crate::modules::operations::domain::entities::OperationProjection;
use crate::modules::workloads::application::{
    DeploymentQueryResult, WorkloadQueryResult, WorkloadReplicaQueryResult,
};
use crate::modules::workloads::domain::entities::{
    PlacementTopology, SkillWorkloadRevisionBinding, WorkloadControl, WorkloadRevision,
};
use crate::modules::workloads::presentation::dto::ServiceTemplateDto;
use a3s_runtime::contract::{RuntimeHealthState, RuntimeUnitState};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub name: String,
    pub desired_state: String,
    pub control: WorkloadControlResponse,
    pub replicas: Vec<WorkloadReplicaResponse>,
    pub desired_revision: Option<WorkloadRevisionResponse>,
    pub active_revision: Option<WorkloadRevisionResponse>,
    pub deployments: Vec<DeploymentResponse>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadControlResponse {
    pub managed_owner: Option<ManagedOwnerResponse>,
    pub placement_policy: EffectivePlacementPolicyResponse,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedOwnerResponse {
    pub kind: String,
    pub owner_id: Uuid,
    pub owner_generation: u64,
    pub owner_spec_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectivePlacementPolicyResponse {
    pub schema: String,
    pub generation: u64,
    pub desired_replicas: u32,
    pub members_per_replica: u32,
    pub topology: String,
    pub replica_anti_affinity: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadReplicaResponse {
    pub id: Uuid,
    pub ordinal: u32,
    pub revision_id: Uuid,
    pub revision_generation: u64,
    pub generation: u64,
    pub lifecycle: String,
    pub evacuation_node_id: Option<Uuid>,
    pub retirement_command_id: Option<Uuid>,
    pub runtime_fenced_at: Option<DateTime<Utc>>,
    pub members: Vec<WorkloadReplicaMemberResponse>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadReplicaMemberResponse {
    pub id: Uuid,
    pub ordinal: u32,
    pub node_id: Option<Uuid>,
    pub placement_generation: u64,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadRevisionResponse {
    pub id: Uuid,
    pub generation: u64,
    pub requested_template: ServiceTemplateDto,
    pub artifact_source_uri: String,
    pub expected_artifact_digest: Option<String>,
    pub request_digest: String,
    pub artifact_uri: Option<String>,
    pub artifact_digest: Option<String>,
    pub artifact_media_type: Option<String>,
    pub template_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_source_revision_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_binding: Option<AgentWorkloadRevisionBindingResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_binding: Option<McpWorkloadRevisionBindingResponse>,
    pub skill_bindings: Vec<SkillWorkloadRevisionBindingResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkloadRevisionBindingResponse {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub build_run_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpWorkloadRevisionBindingResponse {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub profile_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillWorkloadRevisionBindingResponse {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub artifact_size_bytes: u64,
    pub mount_name: String,
    pub mount_target: String,
}

impl From<&SkillWorkloadRevisionBinding> for SkillWorkloadRevisionBindingResponse {
    fn from(binding: &SkillWorkloadRevisionBinding) -> Self {
        Self {
            organization_id: binding.organization_id().as_uuid(),
            asset_id: binding.asset_id().as_uuid(),
            asset_release_id: binding.asset_release_id().as_uuid(),
            artifact_digest: binding.artifact_digest().to_string(),
            artifact_media_type: binding.artifact_media_type().into(),
            artifact_size_bytes: binding.artifact_size_bytes(),
            mount_name: binding.mount_name(),
            mount_target: binding.mount_target(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentResponse {
    pub id: Uuid,
    pub workload_id: Uuid,
    pub replica_id: Uuid,
    pub replica_generation: u64,
    pub member_id: Uuid,
    pub placement_generation: u64,
    pub revision: WorkloadRevisionResponse,
    pub operation_id: Uuid,
    pub node_id: Option<Uuid>,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub command_id: Option<Uuid>,
    pub cleanup_command_id: Option<Uuid>,
    pub retirement_command_id: Option<Uuid>,
    pub status: String,
    pub failure: Option<String>,
    pub operation: Option<DeploymentOperationResponse>,
    pub observed_runtime: Option<ObservedRuntimeResponse>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentOperationResponse {
    pub status: String,
    pub last_sequence: u64,
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedRuntimeResponse {
    pub report_id: Uuid,
    pub node_id: Uuid,
    pub command_id: Option<Uuid>,
    pub unit_id: String,
    pub generation: u64,
    pub spec_digest: String,
    pub state: RuntimeUnitState,
    pub health_state: Option<RuntimeHealthState>,
    pub health_message: Option<String>,
    pub provider_resource_id: Option<String>,
    pub provider_build: Option<String>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

impl From<WorkloadQueryResult> for WorkloadResponse {
    fn from(result: WorkloadQueryResult) -> Self {
        let desired_revision = result.desired_revision().cloned().map(Into::into);
        let active_revision = result.active_revision().cloned().map(Into::into);
        let workload = result.workload;
        Self {
            id: workload.id.as_uuid(),
            organization_id: workload.organization_id.as_uuid(),
            project_id: workload.project_id.as_uuid(),
            environment_id: workload.environment_id.as_uuid(),
            name: workload.name.as_str().to_owned(),
            desired_state: workload.desired_state.as_str().into(),
            control: result.control.into(),
            replicas: result.replicas.into_iter().map(Into::into).collect(),
            desired_revision,
            active_revision,
            deployments: result.deployments.into_iter().map(Into::into).collect(),
            aggregate_version: workload.aggregate_version,
            created_at: workload.created_at,
            updated_at: workload.updated_at,
        }
    }
}

impl From<WorkloadControl> for WorkloadControlResponse {
    fn from(control: WorkloadControl) -> Self {
        let managed_owner = control
            .spec
            .managed_owner
            .map(|owner| ManagedOwnerResponse {
                kind: owner.kind().as_str().to_owned(),
                owner_id: owner.owner_id(),
                owner_generation: owner.owner_generation(),
                owner_spec_digest: owner.owner_spec_digest().to_owned(),
            });
        let policy = control.spec.placement_policy;
        let topology = match policy.topology() {
            PlacementTopology::SingleNode => "single_node",
        };
        Self {
            managed_owner,
            placement_policy: EffectivePlacementPolicyResponse {
                schema: policy.schema().to_owned(),
                generation: policy.generation(),
                desired_replicas: policy.desired_replicas(),
                members_per_replica: policy.members_per_replica(),
                topology: topology.into(),
                replica_anti_affinity: match policy.replica_anti_affinity() {
                    crate::modules::workloads::domain::entities::ReplicaAntiAffinity::Required => {
                        "required".into()
                    }
                },
                digest: policy.digest().to_owned(),
            },
            aggregate_version: control.aggregate_version,
            created_at: control.created_at,
            updated_at: control.updated_at,
        }
    }
}

impl From<WorkloadReplicaQueryResult> for WorkloadReplicaResponse {
    fn from(result: WorkloadReplicaQueryResult) -> Self {
        let replica = result.replica;
        Self {
            id: replica.id.as_uuid(),
            ordinal: replica.ordinal,
            revision_id: replica.revision_id.as_uuid(),
            revision_generation: replica.revision_generation,
            generation: replica.generation,
            lifecycle: replica.lifecycle.as_str().into(),
            evacuation_node_id: replica.evacuation_node_id.map(|node_id| node_id.as_uuid()),
            retirement_command_id: replica
                .retirement_command_id
                .map(|command_id| command_id.as_uuid()),
            runtime_fenced_at: replica.runtime_fenced_at,
            members: result
                .members
                .into_iter()
                .map(|member| WorkloadReplicaMemberResponse {
                    id: member.id.as_uuid(),
                    ordinal: member.ordinal,
                    node_id: member.node_id.map(|node_id| node_id.as_uuid()),
                    placement_generation: member.placement_generation,
                    aggregate_version: member.aggregate_version,
                    created_at: member.created_at,
                    updated_at: member.updated_at,
                })
                .collect(),
            aggregate_version: replica.aggregate_version,
            created_at: replica.created_at,
            updated_at: replica.updated_at,
        }
    }
}

impl From<WorkloadRevision> for WorkloadRevisionResponse {
    fn from(revision: WorkloadRevision) -> Self {
        let external_source_revision_id = revision
            .external_build
            .as_ref()
            .map(|reference| reference.source_revision_id.as_uuid());
        let build_run_id = revision
            .external_build
            .as_ref()
            .map(|reference| reference.build_run_id.as_uuid());
        let agent_binding =
            revision
                .agent_binding()
                .map(|binding| AgentWorkloadRevisionBindingResponse {
                    organization_id: binding.organization_id().as_uuid(),
                    asset_id: binding.asset_id().as_uuid(),
                    asset_release_id: binding.asset_release_id().as_uuid(),
                    build_run_id: binding.build_run_id().as_uuid(),
                });
        let mcp_binding =
            revision
                .mcp_binding()
                .map(|binding| McpWorkloadRevisionBindingResponse {
                    organization_id: binding.organization_id().as_uuid(),
                    asset_id: binding.asset_id().as_uuid(),
                    asset_release_id: binding.asset_release_id().as_uuid(),
                    profile_digest: binding.profile_digest().to_string(),
                });
        let skill_bindings = revision
            .skill_bindings()
            .iter()
            .map(SkillWorkloadRevisionBindingResponse::from)
            .collect();
        let requested_template = revision.request.clone().into();
        let (artifact_uri, artifact_digest, artifact_media_type) = revision
            .template
            .map(|template| {
                (
                    Some(template.artifact.uri),
                    Some(template.artifact.digest),
                    Some(template.artifact.media_type),
                )
            })
            .unwrap_or((None, None, None));
        Self {
            id: revision.id.as_uuid(),
            generation: revision.generation,
            requested_template,
            artifact_source_uri: revision.request.artifact.uri,
            expected_artifact_digest: revision.request.artifact.expected_digest,
            request_digest: revision.request_digest,
            artifact_uri,
            artifact_digest,
            artifact_media_type,
            template_digest: revision.template_digest,
            created_at: revision.created_at,
            resolved_at: revision.resolved_at,
            external_source_revision_id,
            build_run_id,
            agent_binding,
            mcp_binding,
            skill_bindings,
        }
    }
}

impl From<DeploymentQueryResult> for DeploymentResponse {
    fn from(result: DeploymentQueryResult) -> Self {
        let deployment = result.deployment;
        let replica_binding = result.replica_binding;
        Self {
            id: deployment.id.as_uuid(),
            workload_id: deployment.workload_id.as_uuid(),
            replica_id: replica_binding.replica_id.as_uuid(),
            replica_generation: replica_binding.replica_generation,
            member_id: replica_binding.member_id.as_uuid(),
            placement_generation: replica_binding.placement_generation,
            revision: result.revision.into(),
            operation_id: deployment.operation_id.as_uuid(),
            node_id: deployment.node_id.map(|id| id.as_uuid()),
            runtime_unit_id: replica_binding.runtime_unit_id,
            runtime_generation: replica_binding.runtime_generation,
            command_id: deployment.command_id.map(|id| id.as_uuid()),
            cleanup_command_id: deployment.cleanup_command_id.map(|id| id.as_uuid()),
            retirement_command_id: deployment.retirement_command_id.map(|id| id.as_uuid()),
            status: deployment.status.as_str().into(),
            failure: deployment.failure,
            operation: result.operation.map(Into::into),
            observed_runtime: result.observation.map(ObservedRuntimeResponse::from),
            aggregate_version: deployment.aggregate_version,
            requested_at: deployment.requested_at,
            updated_at: deployment.updated_at,
            activated_at: deployment.activated_at,
            cancellation_requested_at: deployment.cancellation_requested_at,
            cancelled_at: deployment.cancelled_at,
        }
    }
}

impl From<OperationProjection> for DeploymentOperationResponse {
    fn from(operation: OperationProjection) -> Self {
        Self {
            status: operation.status.as_str().into(),
            last_sequence: operation.last_sequence,
            error: operation.error,
            updated_at: operation.updated_at,
        }
    }
}

impl From<crate::modules::fleet::domain::repositories::RuntimeObservationRecord>
    for ObservedRuntimeResponse
{
    fn from(record: crate::modules::fleet::domain::repositories::RuntimeObservationRecord) -> Self {
        let observation = record.observation;
        let (health_state, health_message) = observation
            .health
            .map(|health| (Some(health.state), health.message))
            .unwrap_or((None, None));
        let (failure_code, failure_message) = observation
            .failure
            .map(|failure| (Some(failure.code), Some(failure.message)))
            .unwrap_or((None, None));
        Self {
            report_id: record.report_id,
            node_id: record.node_id.as_uuid(),
            command_id: record.command_id.map(|id| id.as_uuid()),
            unit_id: observation.unit_id,
            generation: observation.generation,
            spec_digest: observation.spec_digest,
            state: observation.state,
            health_state,
            health_message,
            provider_resource_id: observation.provider_resource_id,
            provider_build: observation.provider_build,
            failure_code,
            failure_message,
            observed_at: record.observed_at,
            received_at: record.received_at,
        }
    }
}
