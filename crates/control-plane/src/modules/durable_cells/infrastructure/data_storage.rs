use crate::modules::data::{
    ObjectNamespaceCredentialAdmission, ObjectNamespaceCredentialBinding,
    ObjectNamespaceCredentialBindingSpec, ObjectNamespaceKey, ObjectNamespaceProviderProfile,
    ObjectNamespaceRecoveryPoint, ObjectNamespaceRecoveryPointSpec,
    SealObjectNamespaceOperationInput, SealObjectNamespaceOperationOutput,
};
use crate::modules::durable_cells::application::{
    DurableCellStorageCredentialRequest, DurableCellStorageProviderProfileProjection,
    DurableCellStorageProviderProfileRequest, DurableCellStorageRecoveryPointProjection,
    DurableCellStorageSealInputProjection, DurableCellStorageSealRequest, IDurableCellStoragePort,
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
