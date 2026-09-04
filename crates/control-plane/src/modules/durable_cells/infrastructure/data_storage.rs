use crate::modules::data::{
    ObjectNamespaceCredentialAdmission, ObjectNamespaceCredentialBinding,
    ObjectNamespaceCredentialBindingSpec, ObjectNamespaceFlowBinding, ObjectNamespaceKey,
    ObjectNamespaceProviderProfile, ObjectNamespaceRecoveryOperationRequest,
    ObjectNamespaceRecoveryPoint, ObjectNamespaceRecoveryPointSpec, ObjectNamespaceRetentionPolicy,
    ObjectNamespaceRetentionPolicySpec, SealObjectNamespaceOperationInput,
    SealObjectNamespaceOperationOutput,
};
use crate::modules::durable_cells::application::{
    DurableCellStorageCredentialRequest, DurableCellStorageOperationRequestProjection,
    DurableCellStorageProviderProfileProjection, DurableCellStorageProviderProfileRequest,
    DurableCellStorageRecoveryPointProjection, DurableCellStorageRetentionPolicyProjection,
    DurableCellStorageRetentionPolicyRequest, DurableCellStorageRetentionPolicySpec,
    DurableCellStorageSealInputProjection, DurableCellStorageSealOperationRequest,
    DurableCellStorageSealRequest, IDurableCellStoragePort,
};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Anti-corruption adapter from Durable Cells' exact S0 credential identity to
/// Data's credential admission service. Data reconstructs its digest-locked
/// binding and Secrets remains the only active-version authority.
#[derive(Clone)]
pub struct DataDurableCellStorageAdapter {
    credential_admission: ObjectNamespaceCredentialAdmission,
}

impl DataDurableCellStorageAdapter {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self {
            credential_admission: ObjectNamespaceCredentialAdmission::new(secrets),
        }
    }
}

#[async_trait]
impl IDurableCellStoragePort for DataDurableCellStorageAdapter {
    async fn project_provider_profile(
        &self,
        request: &DurableCellStorageProviderProfileRequest,
    ) -> ApplicationResult<DurableCellStorageProviderProfileProjection> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let profile =
            ObjectNamespaceProviderProfile::restore(&request.acl, request.expected_digest.as_str())
                .map_err(|error| {
                    ApplicationError::Internal(format!(
                        "Durable Cell S0 provider profile failed Data validation: {error}"
                    ))
                })?;
        let spec = profile.spec();
        let projection = DurableCellStorageProviderProfileProjection {
            digest: profile.digest().clone(),
            endpoint: spec.endpoint.clone(),
            region: spec.region.clone(),
            bucket: spec.bucket.clone(),
            prefix: spec.prefix.clone(),
            virtual_hosted_style: spec.virtual_hosted_style,
        };
        projection.validate().map_err(ApplicationError::Internal)?;
        if projection.digest != request.expected_digest {
            return Err(ApplicationError::Conflict(
                "Durable Cell S0 provider profile digest changed at the Data boundary".into(),
            ));
        }
        Ok(projection)
    }

    async fn project_retention_policy(
        &self,
        request: &DurableCellStorageRetentionPolicyRequest,
    ) -> ApplicationResult<DurableCellStorageRetentionPolicyProjection> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let policy = ObjectNamespaceRetentionPolicy::restore(
            ObjectNamespaceRetentionPolicySpec {
                minimum_sealed_recovery_points: request.spec.minimum_sealed_recovery_points,
                maximum_sealed_recovery_points: request.spec.maximum_sealed_recovery_points,
                maximum_recovery_point_age_seconds: request.spec.maximum_recovery_point_age_seconds,
                deletion_grace_period_seconds: request.spec.deletion_grace_period_seconds,
            },
            request.expected_digest.as_str(),
        )
        .map_err(|error| {
            ApplicationError::Internal(format!(
                "Durable Cell S0 retention policy failed Data validation: {error}"
            ))
        })?;
        let spec = policy.spec();
        let projection = DurableCellStorageRetentionPolicyProjection {
            spec: DurableCellStorageRetentionPolicySpec {
                minimum_sealed_recovery_points: spec.minimum_sealed_recovery_points,
                maximum_sealed_recovery_points: spec.maximum_sealed_recovery_points,
                maximum_recovery_point_age_seconds: spec.maximum_recovery_point_age_seconds,
                deletion_grace_period_seconds: spec.deletion_grace_period_seconds,
            },
            digest: policy.digest().clone(),
        };
        projection.validate().map_err(ApplicationError::Internal)?;
        if projection.digest != request.expected_digest {
            return Err(ApplicationError::Conflict(
                "Durable Cell S0 retention policy digest changed at the Data boundary".into(),
            ));
        }
        Ok(projection)
    }

    async fn compose_seal_operation(
        &self,
        request: &DurableCellStorageSealOperationRequest,
    ) -> ApplicationResult<DurableCellStorageOperationRequestProjection> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let provider_profile = ObjectNamespaceProviderProfile::restore(
            &request.provider_profile.acl,
            request.provider_profile.expected_digest.as_str(),
        )
        .map_err(|error| {
            ApplicationError::Internal(format!(
                "Durable Cell S0 seal Operation provider profile failed Data validation: {error}"
            ))
        })?;
        let credentials = ObjectNamespaceCredentialBinding::restore(
            ObjectNamespaceCredentialBindingSpec {
                organization_id: request.credentials.organization_id,
                project_id: request.credentials.project_id,
                environment_id: request.credentials.environment_id,
                namespace_id: request.credentials.namespace_id,
                generation: request.credentials.generation,
                provider_profile_digest: request.credentials.provider_profile_digest.clone(),
                access_key_id: request.credentials.access_key_id,
                secret_access_key: request.credentials.secret_access_key,
                session_token: request.credentials.session_token,
            },
            request.credentials.binding_digest.as_str(),
        )
        .map_err(|error| {
            ApplicationError::Internal(format!(
                "Durable Cell S0 seal Operation credential binding failed Data validation: {error}"
            ))
        })?;
        let previous_recovery_point = request
            .previous_recovery_point
            .as_ref()
            .map(restore_recovery_point)
            .transpose()
            .map_err(ApplicationError::Internal)?;
        let operation =
            ObjectNamespaceRecoveryOperationRequest::seal(SealObjectNamespaceOperationInput {
                operation_id: request.seal.operation_id,
                organization_id: request.seal.organization_id,
                source: ObjectNamespaceFlowBinding {
                    provider_profile,
                    credentials,
                },
                previous_recovery_point,
                writer_epoch: request.seal.writer_epoch,
                writer_fence_receipt_digest: request.seal.writer_fence_receipt_digest.clone(),
                sealed_at: request.seal.sealed_at,
            })
            .map_err(|error| {
                ApplicationError::Internal(format!(
                    "Durable Cell S0 seal Operation composition failed Data validation: {error}"
                ))
            })?;
        if operation.subject.kind() != "storage_namespace"
            || operation.subject.id() != request.seal.namespace_id.as_uuid()
            || operation.workflow.name() != "cloud.object-namespace.seal"
            || operation.workflow.version() != "2"
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell S0 seal Operation changed its canonical identity".into(),
            ));
        }
        let projection = DurableCellStorageOperationRequestProjection {
            operation_id: operation.id,
            organization_id: operation.organization_id,
            namespace_id: request.seal.namespace_id,
            workflow_name: operation.workflow.name().to_owned(),
            workflow_version: operation.workflow.version().to_owned(),
            input: operation.input,
            requested_at: operation.requested_at,
        };
        projection.validate().map_err(ApplicationError::Internal)?;
        if projection.operation_id != request.seal.operation_id
            || projection.organization_id != request.seal.organization_id
            || projection.requested_at != request.seal.sealed_at
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell S0 seal Operation changed its exact handoff".into(),
            ));
        }
        Ok(projection)
    }

    async fn require_active_credentials(
        &self,
        request: &DurableCellStorageCredentialRequest,
    ) -> ApplicationResult<()> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let binding = ObjectNamespaceCredentialBinding::restore(
            ObjectNamespaceCredentialBindingSpec {
                organization_id: request.organization_id,
                project_id: request.project_id,
                environment_id: request.environment_id,
                namespace_id: request.namespace_id,
                generation: request.generation,
                provider_profile_digest: request.provider_profile_digest.clone(),
                access_key_id: request.access_key_id,
                secret_access_key: request.secret_access_key,
                session_token: request.session_token,
            },
            request.binding_digest.as_str(),
        )
        .map_err(|error| {
            ApplicationError::Internal(format!(
                "Durable Cell S0 credential projection changed at the Data boundary: {error}"
            ))
        })?;
        self.credential_admission.require_active(&binding).await
    }

    async fn validate_seal_input(
        &self,
        request: &DurableCellStorageSealRequest,
        input: &Value,
    ) -> ApplicationResult<DurableCellStorageSealInputProjection> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let parsed: SealObjectNamespaceOperationInput = serde_json::from_value(input.clone())
            .map_err(|error| {
                ApplicationError::Internal(format!(
                    "Durable Cell S0 seal Operation input is invalid: {error}"
                ))
            })?;
        parsed.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "Durable Cell S0 seal Operation input failed Data validation: {error}"
            ))
        })?;
        let canonical = serde_json::to_value(&parsed).map_err(|error| {
            ApplicationError::Internal(format!(
                "could not canonicalize Durable Cell S0 seal Operation input: {error}"
            ))
        })?;
        if canonical != *input {
            return Err(ApplicationError::Internal(
                "Durable Cell S0 seal Operation input representation drifted".into(),
            ));
        }
        let credentials = parsed.source.credentials.spec();
        if parsed.operation_id != request.operation_id
            || parsed.organization_id != request.organization_id
            || credentials.project_id != request.project_id
            || credentials.environment_id != request.environment_id
            || credentials.namespace_id != request.namespace_id
            || parsed.source.provider_profile.digest() != &request.provider_profile_digest
            || parsed.writer_epoch != request.writer_epoch
            || parsed.writer_fence_receipt_digest != request.writer_fence_receipt_digest
            || parsed.sealed_at != request.sealed_at
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell S0 seal Operation input changed its exact writer-fence identity"
                    .into(),
            ));
        }
        let previous_recovery_point = parsed
            .previous_recovery_point
            .map(project_recovery_point)
            .transpose()
            .map_err(ApplicationError::Internal)?;
        let projection = DurableCellStorageSealInputProjection {
            operation_id: parsed.operation_id,
            organization_id: parsed.organization_id,
            project_id: credentials.project_id,
            environment_id: credentials.environment_id,
            namespace_id: credentials.namespace_id,
            provider_profile_digest: parsed.source.provider_profile.digest().clone(),
            writer_epoch: parsed.writer_epoch,
            writer_fence_receipt_digest: parsed.writer_fence_receipt_digest,
            sealed_at: parsed.sealed_at,
            previous_recovery_point,
        };
        projection
            .validate_against(request)
            .map_err(ApplicationError::Internal)?;
        Ok(projection)
    }

    async fn project_seal_output(
        &self,
        request: &DurableCellStorageSealRequest,
        input: &DurableCellStorageSealInputProjection,
        output: &Value,
    ) -> ApplicationResult<DurableCellStorageRecoveryPointProjection> {
        request.validate().map_err(ApplicationError::Invalid)?;
        input
            .validate_against(request)
            .map_err(ApplicationError::Invalid)?;
        let parsed: SealObjectNamespaceOperationOutput = serde_json::from_value(output.clone())
            .map_err(|error| {
                ApplicationError::Internal(format!(
                    "Durable Cell S0 seal Operation output is invalid: {error}"
                ))
            })?;
        parsed.recovery_point.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "Durable Cell S0 recovery point failed Data validation: {error}"
            ))
        })?;
        let canonical = serde_json::to_value(&parsed).map_err(|error| {
            ApplicationError::Internal(format!(
                "could not canonicalize Durable Cell S0 seal Operation output: {error}"
            ))
        })?;
        if canonical != *output {
            return Err(ApplicationError::Internal(
                "Durable Cell S0 seal Operation output representation drifted".into(),
            ));
        }
        if let Some(previous) = &input.previous_recovery_point {
            let previous = restore_recovery_point(previous).map_err(ApplicationError::Internal)?;
            parsed
                .recovery_point
                .validate_successor_of(&previous)
                .map_err(ApplicationError::Conflict)?;
        }
        let projection =
            project_recovery_point(parsed.recovery_point).map_err(ApplicationError::Internal)?;
        if projection.namespace_id != request.namespace_id
            || projection.provider_profile_digest != request.provider_profile_digest
            || projection.writer_epoch != request.writer_epoch
            || projection.sealed_at < request.sealed_at
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell S0 recovery point changed its exact writer-fence lineage".into(),
            ));
        }
        projection.validate().map_err(ApplicationError::Internal)?;
        Ok(projection)
    }
}

fn project_recovery_point(
    point: ObjectNamespaceRecoveryPoint,
) -> Result<DurableCellStorageRecoveryPointProjection, String> {
    point.validate()?;
    let spec = point.spec();
    let projection = DurableCellStorageRecoveryPointProjection {
        namespace_id: spec.namespace_id,
        sequence: spec.sequence,
        writer_epoch: spec.writer_epoch,
        provider_profile_digest: spec.provider_profile_digest.clone(),
        manifest_key: spec.manifest_key.as_str().to_owned(),
        manifest_digest: spec.manifest_digest.clone(),
        state_digest: spec.state_digest.clone(),
        state_size_bytes: spec.state_size_bytes,
        predecessor_digest: spec.predecessor_digest.clone(),
        sealed_at: spec.sealed_at,
        digest: point.digest().clone(),
    };
    projection.validate()?;
    Ok(projection)
}

fn restore_recovery_point(
    projection: &DurableCellStorageRecoveryPointProjection,
) -> Result<ObjectNamespaceRecoveryPoint, String> {
    projection.validate()?;
    ObjectNamespaceRecoveryPoint::restore(
        ObjectNamespaceRecoveryPointSpec {
            namespace_id: projection.namespace_id,
            sequence: projection.sequence,
            writer_epoch: projection.writer_epoch,
            provider_profile_digest: projection.provider_profile_digest.clone(),
            manifest_key: ObjectNamespaceKey::parse(projection.manifest_key.clone())?,
            manifest_digest: projection.manifest_digest.clone(),
            state_digest: projection.state_digest.clone(),
            state_size_bytes: projection.state_size_bytes,
            predecessor_digest: projection.predecessor_digest.clone(),
            sealed_at: projection.sealed_at,
        },
        projection.digest.as_str(),
    )
}
