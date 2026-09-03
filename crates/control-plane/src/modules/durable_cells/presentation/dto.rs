use crate::modules::durable_cells::domain::{
    DurableCellApplication, DurableCellApplicationRecord, DurableCellApplicationRevision,
    DurableCellDeployment, DURABLE_CELL_APPLICATION_SCHEMA,
};
use crate::modules::durable_cells::{
    DurableCellApplicationMutationResult, DurableCellDeploymentMutationResult,
    DurableCellRoutePublication, DurableCellRoutePublicationResult,
};
use crate::modules::workloads::domain::entities::SkillWorkloadRevisionBinding;
use crate::modules::workloads::domain::repositories::DeploymentBundle;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDurableCellApplicationRequest {
    pub name: String,
    pub definition_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseDurableCellApplicationRequest {
    pub expected_version: u64,
    pub definition_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDurableCellApplicationStateRequest {
    pub expected_version: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeployDurableCellApplicationRequest {
    pub service_profile_acl: String,
    pub storage_provider_profile_acl: Option<String>,
    pub provider_workload_acl: String,
    pub storage_binding_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishDurableCellApplicationRouteRequest {
    pub service_profile_acl: String,
    pub gateway_scope_id: Uuid,
    pub domain_claim_id: Uuid,
    pub hostname: String,
    pub path_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellApplicationResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub application_id: Uuid,
    pub name: String,
    pub desired_state: String,
    pub current_revision_id: Uuid,
    pub current_revision_number: u64,
    pub current_definition_digest: String,
    pub aggregate_version: u64,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DurableCellApplication> for DurableCellApplicationResponse {
    fn from(application: DurableCellApplication) -> Self {
        Self {
            organization_id: application.organization_id.as_uuid(),
            project_id: application.project_id.as_uuid(),
            environment_id: application.environment_id.as_uuid(),
            application_id: application.id.as_uuid(),
            name: application.name.as_str().into(),
            desired_state: application.desired_state.as_str().into(),
            current_revision_id: application.current_revision_id.as_uuid(),
            current_revision_number: application.current_revision_number,
            current_definition_digest: application.current_definition_digest.as_str().into(),
            aggregate_version: application.aggregate_version,
            created_by: application.created_by.as_uuid(),
            created_at: application.created_at,
            updated_at: application.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellApplicationRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub application_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub parent_definition_digest: Option<String>,
    pub definition_schema: String,
    pub definition_acl: String,
    pub definition_digest: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<DurableCellApplicationRevision> for DurableCellApplicationRevisionResponse {
    fn from(revision: DurableCellApplicationRevision) -> Self {
        Self {
            organization_id: revision.organization_id.as_uuid(),
            project_id: revision.project_id.as_uuid(),
            environment_id: revision.environment_id.as_uuid(),
            application_id: revision.application_id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            parent_revision_id: revision.parent_revision_id.map(|value| value.as_uuid()),
            parent_definition_digest: revision
                .parent_definition_digest
                .map(|value| value.as_str().into()),
            definition_schema: DURABLE_CELL_APPLICATION_SCHEMA.into(),
            definition_acl: revision.definition.canonical_acl().into(),
            definition_digest: revision.definition.digest().as_str().into(),
            created_by: revision.created_by.as_uuid(),
            created_at: revision.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellApplicationRecordResponse {
    pub application: DurableCellApplicationResponse,
    pub revision: DurableCellApplicationRevisionResponse,
}

impl From<DurableCellApplicationRecord> for DurableCellApplicationRecordResponse {
    fn from(record: DurableCellApplicationRecord) -> Self {
        Self {
            application: record.application.into(),
            revision: record.revision.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellApplicationMutationResponse {
    pub record: DurableCellApplicationRecordResponse,
    pub replayed: bool,
}

impl From<DurableCellApplicationMutationResult> for DurableCellApplicationMutationResponse {
    fn from(result: DurableCellApplicationMutationResult) -> Self {
        Self {
            record: result.record.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellDeploymentCorrelationResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub application_id: Uuid,
    pub application_revision_id: Uuid,
    pub application_revision_number: u64,
    pub application_definition_digest: String,
    pub storage_namespace_id: Uuid,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub deployment_id: Uuid,
    pub operation_id: Uuid,
    pub service_profile_digest: String,
    pub service_template_digest: String,
    pub provider_artifact_digest: String,
    pub credential_binding_generation: u64,
    pub credential_binding_digest: String,
    pub storage_provider_profile_digest: String,
    pub retention_policy_digest: String,
    pub placement_policy_digest: String,
    pub requested_by: Uuid,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl From<DurableCellDeployment> for DurableCellDeploymentCorrelationResponse {
    fn from(deployment: DurableCellDeployment) -> Self {
        let projection = deployment.projection;
        Self {
            organization_id: projection.organization_id.as_uuid(),
            project_id: projection.project_id.as_uuid(),
            environment_id: projection.environment_id.as_uuid(),
            application_id: projection.application_id.as_uuid(),
            application_revision_id: projection.application_revision_id.as_uuid(),
            application_revision_number: projection.application_revision_number,
            application_definition_digest: projection.application_definition_digest.as_str().into(),
            storage_namespace_id: projection.storage_namespace_id.as_uuid(),
            workload_id: projection.workload_id.as_uuid(),
            workload_revision_id: projection.workload_revision_id.as_uuid(),
            deployment_id: projection.deployment_id.as_uuid(),
            operation_id: projection.operation_id.as_uuid(),
            service_profile_digest: deployment.provider.service_profile_digest.as_str().into(),
            service_template_digest: deployment.provider.service_template_digest.as_str().into(),
            provider_artifact_digest: deployment.provider.provider_artifact_digest.as_str().into(),
            credential_binding_generation: deployment.storage.credential_binding_generation,
            credential_binding_digest: deployment.storage.credential_binding_digest.as_str().into(),
            storage_provider_profile_digest: deployment
                .storage
                .provider_profile_digest
                .as_str()
                .into(),
            retention_policy_digest: deployment.storage.retention_policy_digest.as_str().into(),
            placement_policy_digest: deployment.placement_policy_digest.as_str().into(),
            requested_by: deployment.requested_by.as_uuid(),
            request_id: deployment.request_id,
            requested_at: deployment.requested_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellDeploymentResponse {
    pub correlation: DurableCellDeploymentCorrelationResponse,
    pub workload: DurableCellWorkloadDeploymentResponse,
    pub replayed: bool,
}

impl From<DurableCellDeploymentMutationResult> for DurableCellDeploymentResponse {
    fn from(result: DurableCellDeploymentMutationResult) -> Self {
        Self {
            correlation: result.correlation.into(),
            workload: DurableCellWorkloadDeploymentResponse::from_bundle(result.workload, None),
            replayed: result.replayed,
        }
    }
}

/// Durable Cells' own HTTP projection of the managed Workload result.
///
/// The Workloads presentation DTO is intentionally not reused here: its
/// schema belongs to the Workloads bounded context. This projection preserves
/// the established wire shape while keeping the Durable Cells presentation
/// boundary responsible for its own response contract.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellWorkloadDeploymentResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub workload_id: Uuid,
    pub revision_id: Uuid,
    pub deployment_id: Uuid,
    pub operation_id: Uuid,
    pub generation: u64,
    pub status: String,
    pub artifact_source_uri: String,
    pub expected_artifact_digest: Option<String>,
    pub request_digest: String,
    pub artifact_digest: Option<String>,
    pub template_digest: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub replayed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_source_revision_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_source_revision_id: Option<Uuid>,
    pub skill_bindings: Vec<DurableCellSkillWorkloadRevisionBindingResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellSkillWorkloadRevisionBindingResponse {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub artifact_size_bytes: u64,
    pub mount_name: String,
    pub mount_target: String,
}

impl From<&SkillWorkloadRevisionBinding> for DurableCellSkillWorkloadRevisionBindingResponse {
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

impl DurableCellWorkloadDeploymentResponse {
    fn from_bundle(bundle: DeploymentBundle, rollback_source_revision_id: Option<Uuid>) -> Self {
        let external_source_revision_id = bundle
            .revision
            .external_build
            .as_ref()
            .map(|reference| reference.source_revision_id.as_uuid());
        let build_run_id = bundle
            .revision
            .external_build
            .as_ref()
            .map(|reference| reference.build_run_id.as_uuid());
        let skill_bindings = bundle
            .revision
            .skill_bindings()
            .iter()
            .map(DurableCellSkillWorkloadRevisionBindingResponse::from)
            .collect();
        Self {
            organization_id: bundle.workload.organization_id.as_uuid(),
            project_id: bundle.workload.project_id.as_uuid(),
            environment_id: bundle.workload.environment_id.as_uuid(),
            workload_id: bundle.workload.id.as_uuid(),
            revision_id: bundle.revision.id.as_uuid(),
            deployment_id: bundle.deployment.id.as_uuid(),
            operation_id: bundle.operation.id.as_uuid(),
            generation: bundle.revision.generation,
            status: bundle.deployment.status.as_str().into(),
            artifact_source_uri: bundle.revision.request.artifact.uri,
            expected_artifact_digest: bundle.revision.request.artifact.expected_digest,
            request_digest: bundle.revision.request_digest,
            artifact_digest: bundle
                .revision
                .template
                .map(|template| template.artifact.digest),
            template_digest: bundle.revision.template_digest,
            requested_at: bundle.deployment.requested_at,
            replayed: bundle.replayed,
            external_source_revision_id,
            build_run_id,
            rollback_source_revision_id,
            skill_bindings,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellRoutePublicationResponse {
    pub correlation: DurableCellDeploymentCorrelationResponse,
    pub publication: DurableCellRoutePublication,
}

impl From<DurableCellRoutePublicationResult> for DurableCellRoutePublicationResponse {
    fn from(result: DurableCellRoutePublicationResult) -> Self {
        Self {
            correlation: result.correlation.into(),
            publication: result.publication,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_request_preserves_the_pre_c3b_payload_shape() {
        let request: DeployDurableCellApplicationRequest =
            serde_json::from_value(serde_json::json!({
                "serviceProfileAcl": "service",
                "providerWorkloadAcl": "workload",
                "storageBindingAcl": "storage"
            }))
            .expect("legacy Durable Cell deployment request");

        assert!(request.storage_provider_profile_acl.is_none());
    }
}
