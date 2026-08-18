use crate::modules::data::domain::{
    ObjectNamespaceCredentialBinding, ObjectNamespaceDeletionEvidence, ObjectNamespaceDeletionPlan,
    ObjectNamespaceProviderProfile, ObjectNamespaceRecoveryPoint, ObjectNamespaceRestoreEvidence,
    ObjectNamespaceRestorePlan, ObjectNamespaceRetentionPolicy,
};
use crate::modules::operations::{OperationRequest, OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OperationId, OrganizationId, Sha256Digest, StorageNamespaceId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME: &str = "cloud.object-namespace.seal";
pub const OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME: &str = "cloud.object-namespace.restore";
pub const OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME: &str = "cloud.object-namespace.delete";
pub const OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION: &str = "1";

/// Exact non-secret provider and credential-reference input persisted by
/// Operations/Flow. Secret plaintext is materialized only inside an S0 step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectNamespaceFlowBinding {
    pub provider_profile: ObjectNamespaceProviderProfile,
    pub credentials: ObjectNamespaceCredentialBinding,
}

impl ObjectNamespaceFlowBinding {
    fn validate_for(
        &self,
        organization_id: OrganizationId,
        namespace_id: StorageNamespaceId,
    ) -> Result<(), String> {
        self.provider_profile.validate()?;
        self.credentials
            .validate_provider_profile(&self.provider_profile)?;
        let scope = self.credentials.spec();
        if scope.organization_id != organization_id || scope.namespace_id != namespace_id {
            return Err("object namespace Flow binding has the wrong exact scope".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SealObjectNamespaceOperationInput {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub source: ObjectNamespaceFlowBinding,
    pub previous_recovery_point: Option<ObjectNamespaceRecoveryPoint>,
    pub writer_epoch: u64,
    pub writer_fence_receipt_digest: Sha256Digest,
    pub sealed_at: DateTime<Utc>,
}

impl SealObjectNamespaceOperationInput {
    pub fn validate(&self) -> Result<(), String> {
        validate_operation_identity(self.operation_id, self.organization_id)?;
        let namespace_id = self.source.credentials.spec().namespace_id;
        self.source
            .validate_for(self.organization_id, namespace_id)?;
        if self.writer_epoch == 0
            || self.sealed_at != canonical_timestamp(self.sealed_at)
            || Sha256Digest::parse(self.writer_fence_receipt_digest.as_str())?
                != self.writer_fence_receipt_digest
        {
            return Err("object namespace seal operation input is invalid".into());
        }
        if let Some(previous) = &self.previous_recovery_point {
            previous.validate()?;
            if previous.spec().namespace_id != namespace_id
                || previous.spec().provider_profile_digest != *self.source.provider_profile.digest()
                || self.writer_epoch < previous.spec().writer_epoch
                || self.sealed_at < previous.spec().sealed_at
            {
                return Err(
                    "object namespace seal operation changed or regressed its predecessor".into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreObjectNamespaceOperationInput {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub source: ObjectNamespaceFlowBinding,
    pub target: ObjectNamespaceFlowBinding,
    pub recovery_point: ObjectNamespaceRecoveryPoint,
    pub restore_plan: ObjectNamespaceRestorePlan,
    pub retention_policy: ObjectNamespaceRetentionPolicy,
}

impl RestoreObjectNamespaceOperationInput {
    pub fn validate(&self) -> Result<(), String> {
        validate_operation_identity(self.operation_id, self.organization_id)?;
        self.recovery_point.validate()?;
        self.restore_plan
            .validate_source(&self.recovery_point, &self.retention_policy)?;
        let plan = self.restore_plan.spec();
        self.source
            .validate_for(self.organization_id, plan.source_namespace_id)?;
        self.target
            .validate_for(self.organization_id, plan.target_namespace_id)?;
        validate_common_scope(&self.source, &self.target)?;
        if self.source.provider_profile.digest() != &plan.source_provider_profile_digest
            || self.target.provider_profile.digest() != &plan.target_provider_profile_digest
        {
            return Err("object namespace restore operation changed a provider profile".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteObjectNamespaceOperationInput {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub source: ObjectNamespaceFlowBinding,
    pub retained_restore: ObjectNamespaceFlowBinding,
    pub recovery_point: ObjectNamespaceRecoveryPoint,
    pub restore_plan: ObjectNamespaceRestorePlan,
    pub restore_evidence: ObjectNamespaceRestoreEvidence,
    pub deletion_plan: ObjectNamespaceDeletionPlan,
    pub retention_policy: ObjectNamespaceRetentionPolicy,
}

impl DeleteObjectNamespaceOperationInput {
    pub fn validate(&self) -> Result<(), String> {
        validate_operation_identity(self.operation_id, self.organization_id)?;
        self.restore_plan
            .validate_source(&self.recovery_point, &self.retention_policy)?;
        self.restore_evidence.validate_for(&self.restore_plan)?;
        self.deletion_plan.validate_against(
            &self.recovery_point,
            &self.restore_plan,
            &self.restore_evidence,
            &self.retention_policy,
        )?;
        let plan = self.deletion_plan.spec();
        self.source
            .validate_for(self.organization_id, plan.namespace_id)?;
        self.retained_restore
            .validate_for(self.organization_id, plan.retained_restore_namespace_id)?;
        validate_common_scope(&self.source, &self.retained_restore)?;
        if self.source.provider_profile.digest() != &plan.provider_profile_digest
            || self.retained_restore.provider_profile.digest()
                != &self.restore_plan.spec().target_provider_profile_digest
        {
            return Err("object namespace deletion operation changed a provider profile".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SealObjectNamespaceOperationOutput {
    pub recovery_point: ObjectNamespaceRecoveryPoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreObjectNamespaceOperationOutput {
    pub restore_evidence: ObjectNamespaceRestoreEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteObjectNamespaceOperationOutput {
    pub deletion_evidence: ObjectNamespaceDeletionEvidence,
}

/// Builds the sole Operations request shape for S0 recovery work. Owning
/// aggregates persist/enqueue the returned request atomically with their own
/// state; this builder does not add an S0 operation repository.
pub struct ObjectNamespaceRecoveryOperationRequest;

impl ObjectNamespaceRecoveryOperationRequest {
    pub fn seal(input: SealObjectNamespaceOperationInput) -> Result<OperationRequest, String> {
        input.validate()?;
        build_request(
            input.operation_id,
            input.organization_id,
            input.source.credentials.spec().namespace_id,
            OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
            input.sealed_at,
            &input,
        )
    }

    pub fn restore(
        input: RestoreObjectNamespaceOperationInput,
    ) -> Result<OperationRequest, String> {
        input.validate()?;
        build_request(
            input.operation_id,
            input.organization_id,
            input.restore_plan.spec().source_namespace_id,
            OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
            input.restore_plan.spec().requested_at,
            &input,
        )
    }

    pub fn delete(input: DeleteObjectNamespaceOperationInput) -> Result<OperationRequest, String> {
        input.validate()?;
        build_request(
            input.operation_id,
            input.organization_id,
            input.deletion_plan.spec().namespace_id,
            OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
            input.deletion_plan.spec().requested_at,
            &input,
        )
    }
}

fn build_request<T: Serialize>(
    operation_id: OperationId,
    organization_id: OrganizationId,
    namespace_id: StorageNamespaceId,
    workflow_name: &str,
    requested_at: DateTime<Utc>,
    input: &T,
) -> Result<OperationRequest, String> {
    Ok(OperationRequest::new(
        operation_id,
        organization_id,
        OperationSubject::new("storage_namespace", namespace_id.as_uuid())?,
        WorkflowIdentity::new(workflow_name, OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION)?,
        serde_json::to_value(input)
            .map_err(|error| format!("could not encode object namespace operation: {error}"))?,
        requested_at,
    ))
}

fn validate_operation_identity(
    operation_id: OperationId,
    organization_id: OrganizationId,
) -> Result<(), String> {
    if operation_id.as_uuid().is_nil() || organization_id.as_uuid().is_nil() {
        return Err("object namespace operation identity is invalid".into());
    }
    Ok(())
}

fn validate_common_scope(
    source: &ObjectNamespaceFlowBinding,
    target: &ObjectNamespaceFlowBinding,
) -> Result<(), String> {
    let source = source.credentials.spec();
    let target = target.credentials.spec();
    if source.organization_id != target.organization_id
        || source.project_id != target.project_id
        || source.environment_id != target.environment_id
    {
        return Err("object namespace operation cannot cross a tenant project environment".into());
    }
    Ok(())
}
