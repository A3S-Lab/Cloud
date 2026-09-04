use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, EnvironmentId, OperationId, OrganizationId,
    ProjectId, SecretVersionReference, Sha256Digest, StorageNamespaceId,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;

const MAX_CREDENTIAL_BINDING_BYTES: usize = 16 * 1024;
const MAX_PROVIDER_PROFILE_ACL_BYTES: usize = 16 * 1024;
const MAX_PROVIDER_PROFILE_FIELD_BYTES: usize = 4096;
const MAX_RETENTION_POLICY_BYTES: usize = 4 * 1024;
const MAX_RECOVERY_POINTS: u32 = 10_000;
const MINIMUM_RECOVERY_POINT_AGE_SECONDS: u64 = 60 * 60;
const MAXIMUM_RECOVERY_POINT_AGE_SECONDS: u64 = 10 * 365 * 24 * 60 * 60;
const MINIMUM_DELETION_GRACE_SECONDS: u64 = 5 * 60;
const MAXIMUM_DELETION_GRACE_SECONDS: u64 = 30 * 24 * 60 * 60;
const MAX_RECOVERY_POINT_BYTES: usize = 32 * 1024;
const MAX_RECOVERY_POINT_KEY_BYTES: usize = 4096;
const MAX_SAFE_SERIALIZED_INTEGER: u64 = 9_007_199_254_740_991;

/// Plaintext-free, consumer-owned identity for one exact S0 credential
/// binding. Data and Secrets retain validation, revocation, and materialization
/// authority; Durable Cells carries only the scope and immutable references it
/// needs to request admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageCredentialRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub namespace_id: StorageNamespaceId,
    pub generation: u64,
    pub provider_profile_digest: Sha256Digest,
    pub access_key_id: SecretVersionReference,
    pub secret_access_key: SecretVersionReference,
    pub session_token: Option<SecretVersionReference>,
    pub binding_digest: Sha256Digest,
}

impl DurableCellStorageCredentialRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        namespace_id: StorageNamespaceId,
        generation: u64,
        provider_profile_digest: Sha256Digest,
        access_key_id: SecretVersionReference,
        secret_access_key: SecretVersionReference,
        session_token: Option<SecretVersionReference>,
    ) -> Result<Self, String> {
        let mut request = Self {
            organization_id,
            project_id,
            environment_id,
            namespace_id,
            generation,
            provider_profile_digest,
            access_key_id,
            secret_access_key,
            session_token,
            binding_digest: Sha256Digest::from_bytes(&[]),
        };
        request.binding_digest = request.expected_binding_digest()?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.namespace_id.as_uuid().is_nil()
            || self.generation == 0
            || Sha256Digest::parse(self.provider_profile_digest.as_str())?
                != self.provider_profile_digest
            || Sha256Digest::parse(self.binding_digest.as_str())? != self.binding_digest
            || self.expected_binding_digest()? != self.binding_digest
        {
            return Err("Durable Cell S0 credential identity is invalid".into());
        }
        self.access_key_id.validate()?;
        self.secret_access_key.validate()?;
        if let Some(session_token) = self.session_token {
            session_token.validate()?;
        }
        let mut secret_ids = self
            .references()
            .into_iter()
            .map(|reference| reference.secret_id)
            .collect::<Vec<_>>();
        secret_ids.sort_unstable();
        secret_ids.dedup();
        if secret_ids.len() != self.references().len() {
            return Err("Durable Cell S0 credential fields must use distinct Secrets".into());
        }
        Ok(())
    }

    pub fn references(&self) -> Vec<SecretVersionReference> {
        let mut references = vec![self.access_key_id, self.secret_access_key];
        if let Some(session_token) = self.session_token {
            references.push(session_token);
        }
        references
    }

    fn expected_binding_digest(&self) -> Result<Sha256Digest, String> {
        let bytes = canonical_json_bounded(
            &DurableCellStorageCredentialIdentity {
                organization_id: self.organization_id,
                project_id: self.project_id,
                environment_id: self.environment_id,
                namespace_id: self.namespace_id,
                generation: self.generation,
                provider_profile_digest: &self.provider_profile_digest,
                access_key_id: self.access_key_id,
                secret_access_key: self.secret_access_key,
                session_token: self.session_token,
            },
            MAX_CREDENTIAL_BINDING_BYTES,
            "Durable Cell S0 credential identity",
        )?;
        Ok(Sha256Digest::from_bytes(&bytes))
    }
}

/// Opaque, immutable S0 provider-profile input accepted by the Durable Cells
/// consumer port. Data remains the authority for parsing the canonical ACL and
/// validating provider semantics; this request carries only the ACL bytes and
/// the digest already bound to the Durable Cell deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageProviderProfileRequest {
    pub acl: String,
    pub expected_digest: Sha256Digest,
}

impl DurableCellStorageProviderProfileRequest {
    pub fn new(acl: impl Into<String>, expected_digest: Sha256Digest) -> Result<Self, String> {
        let request = Self {
            acl: acl.into(),
            expected_digest,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.acl.is_empty()
            || self.acl.len() > MAX_PROVIDER_PROFILE_ACL_BYTES
            // Canonical ACL is a bounded, line-oriented document; newlines
            // are part of its wire representation. NUL is the only control
            // byte rejected before the consumer-owned parser runs.
            || self.acl.contains('\0')
            || Sha256Digest::parse(self.expected_digest.as_str())? != self.expected_digest
        {
            return Err("Durable Cell S0 provider-profile request is invalid".into());
        }
        Ok(())
    }
}

/// Immutable, provider-neutral S0 profile semantics required by the pinned
/// Durable Cell adapter. No credential, client, repository, or namespace
/// lifecycle crosses this projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellStorageProviderProfileProjection {
    pub digest: Sha256Digest,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub virtual_hosted_style: bool,
}

impl DurableCellStorageProviderProfileProjection {
    pub fn validate(&self) -> Result<(), String> {
        if Sha256Digest::parse(self.digest.as_str())? != self.digest
            || self.endpoint.is_empty()
            || self.endpoint.len() > MAX_PROVIDER_PROFILE_FIELD_BYTES
            || self.region.is_empty()
            || self.region.len() > MAX_PROVIDER_PROFILE_FIELD_BYTES
            || self.bucket.is_empty()
            || self.bucket.len() > MAX_PROVIDER_PROFILE_FIELD_BYTES
            || self.prefix.is_empty()
            || self.prefix.len() > MAX_PROVIDER_PROFILE_FIELD_BYTES
            || [
                self.endpoint.as_str(),
                self.region.as_str(),
                self.bucket.as_str(),
                self.prefix.as_str(),
            ]
            .iter()
            .any(|value| value.contains(['\0', '\r', '\n']))
        {
            return Err("Durable Cell S0 provider-profile projection is invalid".into());
        }
        canonical_json_bounded(
            self,
            MAX_PROVIDER_PROFILE_ACL_BYTES,
            "Durable Cell S0 provider-profile projection",
        )?;
        Ok(())
    }

    pub fn namespace_prefix(&self, namespace_id: StorageNamespaceId) -> Result<String, String> {
        self.validate()?;
        if namespace_id.as_uuid().is_nil() {
            return Err("Durable Cell S0 namespace requires a non-nil namespace ID".into());
        }
        Ok(format!("{}/{}", self.prefix, namespace_id))
    }

    pub fn recovery_prefix(&self, namespace_id: StorageNamespaceId) -> Result<String, String> {
        self.validate()?;
        if namespace_id.as_uuid().is_nil() {
            return Err("Durable Cell S0 recovery scope requires a non-nil namespace ID".into());
        }
        Ok(format!("{}/.a3s-recovery/{namespace_id}", self.prefix))
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

/// Provider-neutral retention values accepted at the Durable Cells Storage
/// boundary. The Data owner remains responsible for interpreting the policy;
/// this value contains only the bounded immutable numbers needed by Cloud
/// correlation and recovery decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellStorageRetentionPolicySpec {
    pub minimum_sealed_recovery_points: u32,
    pub maximum_sealed_recovery_points: u32,
    pub maximum_recovery_point_age_seconds: u64,
    pub deletion_grace_period_seconds: u64,
}

impl DurableCellStorageRetentionPolicySpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.minimum_sealed_recovery_points == 0
            || self.maximum_sealed_recovery_points < self.minimum_sealed_recovery_points
            || self.maximum_sealed_recovery_points > MAX_RECOVERY_POINTS
            || !(MINIMUM_RECOVERY_POINT_AGE_SECONDS..=MAXIMUM_RECOVERY_POINT_AGE_SECONDS)
                .contains(&self.maximum_recovery_point_age_seconds)
            || !(MINIMUM_DELETION_GRACE_SECONDS..=MAXIMUM_DELETION_GRACE_SECONDS)
                .contains(&self.deletion_grace_period_seconds)
        {
            return Err("Durable Cell S0 retention policy is outside supported bounds".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<Sha256Digest, String> {
        let bytes = canonical_json_bounded(
            self,
            MAX_RETENTION_POLICY_BYTES,
            "Durable Cell S0 retention policy",
        )?;
        Ok(Sha256Digest::from_bytes(&bytes))
    }
}

/// Exact retention-policy input supplied by a Durable Cells consumer. The
/// expected digest is carried separately so an owner adapter can reject a
/// substituted or re-normalized policy before returning a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageRetentionPolicyRequest {
    pub spec: DurableCellStorageRetentionPolicySpec,
    pub expected_digest: Sha256Digest,
}

impl DurableCellStorageRetentionPolicyRequest {
    pub fn new(
        spec: DurableCellStorageRetentionPolicySpec,
        expected_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let request = Self {
            spec,
            expected_digest,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.spec.validate()?;
        if Sha256Digest::parse(self.expected_digest.as_str())? != self.expected_digest
            || self.spec.digest()? != self.expected_digest
        {
            return Err("Durable Cell S0 retention policy digest is not canonical".into());
        }
        Ok(())
    }
}

/// Immutable retention projection returned by the S0 owner adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DurableCellStorageRetentionPolicyProjection {
    pub spec: DurableCellStorageRetentionPolicySpec,
    pub digest: Sha256Digest,
}

impl DurableCellStorageRetentionPolicyProjection {
    pub fn validate(&self) -> Result<(), String> {
        self.spec.validate()?;
        if Sha256Digest::parse(self.digest.as_str())? != self.digest
            || self.spec.digest()? != self.digest
        {
            return Err("Durable Cell S0 retention projection digest drifted".into());
        }
        Ok(())
    }

    pub fn deletion_not_before(
        &self,
        requested_at: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, String> {
        self.validate()?;
        requested_at
            .checked_add_signed(Duration::seconds(
                self.spec.deletion_grace_period_seconds as i64,
            ))
            .ok_or_else(|| "Durable Cell S0 deletion grace period overflowed".into())
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

/// Exact writer-fence identity accepted by the S0 recovery projection. The
/// Data adapter validates the persisted Operation input/output against this
/// request before returning any recovery evidence to Durable Cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageSealRequest {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub namespace_id: StorageNamespaceId,
    pub provider_profile_digest: Sha256Digest,
    pub writer_epoch: u64,
    pub writer_fence_receipt_digest: Sha256Digest,
    pub sealed_at: DateTime<Utc>,
}

impl DurableCellStorageSealRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        namespace_id: StorageNamespaceId,
        provider_profile_digest: Sha256Digest,
        writer_epoch: u64,
        writer_fence_receipt_digest: Sha256Digest,
        sealed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            project_id,
            environment_id,
            namespace_id,
            provider_profile_digest,
            writer_epoch,
            writer_fence_receipt_digest,
            sealed_at,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.operation_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.namespace_id.as_uuid().is_nil()
            || self.writer_epoch == 0
            || self.writer_epoch > MAX_SAFE_SERIALIZED_INTEGER
            || self.sealed_at != canonical_timestamp(self.sealed_at)
            || Sha256Digest::parse(self.provider_profile_digest.as_str())?
                != self.provider_profile_digest
            || Sha256Digest::parse(self.writer_fence_receipt_digest.as_str())?
                != self.writer_fence_receipt_digest
        {
            return Err("Durable Cell S0 seal request identity is invalid".into());
        }
        Ok(())
    }
}

/// Immutable, owner-neutral projection of one Data/S0 recovery point. The
/// provider-specific object key is retained as a bounded string, while the
/// Data aggregate and its private digest calculation remain behind the owner
/// adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellStorageRecoveryPointProjection {
    pub namespace_id: StorageNamespaceId,
    pub sequence: u64,
    pub writer_epoch: u64,
    pub provider_profile_digest: Sha256Digest,
    pub manifest_key: String,
    pub manifest_digest: Sha256Digest,
    pub state_digest: Sha256Digest,
    pub state_size_bytes: u64,
    pub predecessor_digest: Option<Sha256Digest>,
    pub sealed_at: DateTime<Utc>,
    pub digest: Sha256Digest,
}

impl DurableCellStorageRecoveryPointProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.namespace_id.as_uuid().is_nil()
            || self.sequence == 0
            || self.sequence > MAX_SAFE_SERIALIZED_INTEGER
            || self.writer_epoch == 0
            || self.writer_epoch > MAX_SAFE_SERIALIZED_INTEGER
            || self.state_size_bytes == 0
            || self.state_size_bytes > MAX_SAFE_SERIALIZED_INTEGER
            || self.manifest_key.is_empty()
            || self.manifest_key.len() > MAX_RECOVERY_POINT_KEY_BYTES
            || self.manifest_key.contains(['\\', '\0', '\r', '\n'])
            || self.manifest_key.starts_with('/')
            || self
                .manifest_key
                .split('/')
                .any(|component| component.is_empty() || component == "." || component == "..")
            || self.sealed_at != canonical_timestamp(self.sealed_at)
            || Sha256Digest::parse(self.provider_profile_digest.as_str())?
                != self.provider_profile_digest
            || Sha256Digest::parse(self.manifest_digest.as_str())? != self.manifest_digest
            || Sha256Digest::parse(self.state_digest.as_str())? != self.state_digest
            || Sha256Digest::parse(self.digest.as_str())? != self.digest
        {
            return Err("Durable Cell S0 recovery point projection is invalid".into());
        }
        match (&self.predecessor_digest, self.sequence) {
            (None, 1) => {}
            (Some(predecessor), sequence) if sequence > 1 => {
                if Sha256Digest::parse(predecessor.as_str())? != *predecessor {
                    return Err("Durable Cell S0 predecessor digest is not canonical".into());
                }
            }
            _ => return Err("Durable Cell S0 recovery lineage is invalid".into()),
        }
        canonical_json_bounded(
            self,
            MAX_RECOVERY_POINT_BYTES,
            "Durable Cell S0 recovery point projection",
        )?;
        Ok(())
    }

    pub fn validate_successor_of(&self, previous: &Self) -> Result<(), String> {
        self.validate()?;
        previous.validate()?;
        if self.namespace_id != previous.namespace_id
            || self.sequence
                != previous
                    .sequence
                    .checked_add(1)
                    .ok_or_else(|| "Durable Cell S0 recovery sequence is exhausted".to_owned())?
            || self.predecessor_digest.as_ref() != Some(&previous.digest)
            || self.writer_epoch < previous.writer_epoch
            || self.sealed_at < previous.sealed_at
        {
            return Err("Durable Cell S0 recovery point is not an exact successor".into());
        }
        Ok(())
    }
}

/// Typed, owner-neutral result of validating a persisted S0 seal Operation
/// input. Data owns the concrete Flow binding and recovery aggregate; only
/// these exact lineage fields cross into Durable Cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageSealInputProjection {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub namespace_id: StorageNamespaceId,
    pub provider_profile_digest: Sha256Digest,
    pub writer_epoch: u64,
    pub writer_fence_receipt_digest: Sha256Digest,
    pub sealed_at: DateTime<Utc>,
    pub previous_recovery_point: Option<DurableCellStorageRecoveryPointProjection>,
}

impl DurableCellStorageSealInputProjection {
    pub fn validate_against(&self, request: &DurableCellStorageSealRequest) -> Result<(), String> {
        request.validate()?;
        if self.operation_id != request.operation_id
            || self.organization_id != request.organization_id
            || self.project_id != request.project_id
            || self.environment_id != request.environment_id
            || self.namespace_id != request.namespace_id
            || self.provider_profile_digest != request.provider_profile_digest
            || self.writer_epoch != request.writer_epoch
            || self.writer_fence_receipt_digest != request.writer_fence_receipt_digest
            || self.sealed_at != request.sealed_at
        {
            return Err("Durable Cell S0 seal input crossed its exact scope".into());
        }
        if let Some(previous) = &self.previous_recovery_point {
            previous.validate()?;
            if previous.namespace_id != self.namespace_id
                || previous.provider_profile_digest != self.provider_profile_digest
                || previous.writer_epoch > self.writer_epoch
                || previous.sealed_at > self.sealed_at
            {
                return Err("Durable Cell S0 seal predecessor crossed its exact lineage".into());
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct DurableCellStorageCredentialIdentity<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    namespace_id: StorageNamespaceId,
    generation: u64,
    provider_profile_digest: &'a Sha256Digest,
    access_key_id: SecretVersionReference,
    secret_access_key: SecretVersionReference,
    session_token: Option<SecretVersionReference>,
}

/// Durable Cells' sole mutable admission boundary for its S0 binding.
/// Implementations may consult Data and Secrets owner services, but no owner
/// repository, plaintext, or credential lifecycle crosses this interface.
#[async_trait]
pub trait IDurableCellStoragePort: Send + Sync {
    /// Resolves one canonical Data/S0 provider-profile ACL into the bounded
    /// semantics needed by Durable Cell publication. The owner adapter is the
    /// only place that parses or restores the concrete profile aggregate.
    async fn project_provider_profile(
        &self,
        request: &DurableCellStorageProviderProfileRequest,
    ) -> ApplicationResult<DurableCellStorageProviderProfileProjection>;

    /// Resolves the immutable retention values bound to one S0 namespace.
    /// Data remains the policy parser and digest authority; Durable Cells
    /// receives only this bounded provider-neutral projection.
    async fn project_retention_policy(
        &self,
        request: &DurableCellStorageRetentionPolicyRequest,
    ) -> ApplicationResult<DurableCellStorageRetentionPolicyProjection>;

    async fn require_active_credentials(
        &self,
        request: &DurableCellStorageCredentialRequest,
    ) -> ApplicationResult<()>;

    /// Parses and validates the Data-owned seal input persisted by
    /// Operations. The raw JSON is accepted only at this adapter boundary so
    /// Durable Cells never imports Data's operation request model.
    async fn validate_seal_input(
        &self,
        request: &DurableCellStorageSealRequest,
        input: &Value,
    ) -> ApplicationResult<DurableCellStorageSealInputProjection>;

    /// Parses a successful Data-owned seal output and returns only the exact
    /// immutable recovery-point projection required by the writer gate.
    async fn project_seal_output(
        &self,
        request: &DurableCellStorageSealRequest,
        input: &DurableCellStorageSealInputProjection,
        output: &Value,
    ) -> ApplicationResult<DurableCellStorageRecoveryPointProjection>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OperationId, SecretId};
    use chrono::Duration;

    fn reference() -> SecretVersionReference {
        SecretVersionReference::new(SecretId::new(), 1).expect("Secret reference")
    }

    fn request() -> DurableCellStorageCredentialRequest {
        DurableCellStorageCredentialRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            StorageNamespaceId::new(),
            1,
            Sha256Digest::from_bytes(b"provider-profile"),
            reference(),
            reference(),
            Some(reference()),
        )
        .expect("credential request")
    }

    #[test]
    fn request_is_exactly_scoped_and_digest_locked() {
        let request = request();
        request.validate().expect("valid request");

        let mut drifted = request.clone();
        drifted.generation += 1;
        assert!(drifted.validate().is_err());
    }

    #[test]
    fn request_rejects_secret_aliasing() {
        let request = request();
        assert!(DurableCellStorageCredentialRequest::new(
            request.organization_id,
            request.project_id,
            request.environment_id,
            request.namespace_id,
            request.generation,
            request.provider_profile_digest,
            request.access_key_id,
            SecretVersionReference::new(request.access_key_id.secret_id, 2)
                .expect("aliased Secret reference"),
            request.session_token,
        )
        .is_err());
    }

    fn provider_profile() -> DurableCellStorageProviderProfileProjection {
        DurableCellStorageProviderProfileProjection {
            digest: Sha256Digest::from_bytes(b"provider-profile"),
            endpoint: "https://s3.example.test/".into(),
            region: "us-east-1".into(),
            bucket: "a3s-test".into(),
            prefix: "durable-cells".into(),
            virtual_hosted_style: false,
        }
    }

    fn retention_spec() -> DurableCellStorageRetentionPolicySpec {
        DurableCellStorageRetentionPolicySpec {
            minimum_sealed_recovery_points: 2,
            maximum_sealed_recovery_points: 24,
            maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
            deletion_grace_period_seconds: 24 * 60 * 60,
        }
    }

    #[test]
    fn provider_profile_projection_is_bounded_and_derives_disjoint_scopes() {
        let profile = provider_profile();
        profile.validate().expect("provider profile projection");
        let namespace = StorageNamespaceId::new();
        assert_eq!(
            profile
                .namespace_prefix(namespace)
                .expect("namespace prefix"),
            format!("durable-cells/{namespace}")
        );
        assert_eq!(
            profile.recovery_prefix(namespace).expect("recovery prefix"),
            format!("durable-cells/.a3s-recovery/{namespace}")
        );
        let mut invalid = profile;
        invalid.prefix.clear();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn provider_profile_request_requires_a_canonical_digest_bound_acl() {
        let request = DurableCellStorageProviderProfileRequest::new(
            "object_namespace_provider {}",
            Sha256Digest::from_bytes(b"profile"),
        )
        .expect("profile request");
        request.validate().expect("valid profile request");
        let mut invalid = request;
        invalid.acl.push('\0');
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn retention_projection_is_digest_locked_and_exposes_delete_grace() {
        let spec = retention_spec();
        let request = DurableCellStorageRetentionPolicyRequest::new(
            spec,
            spec.digest().expect("retention digest"),
        )
        .expect("retention request");
        request.validate().expect("valid retention request");
        let projection = DurableCellStorageRetentionPolicyProjection {
            spec,
            digest: request.expected_digest.clone(),
        };
        projection.validate().expect("retention projection");
        let now = canonical_timestamp(Utc::now());
        assert_eq!(
            projection.deletion_not_before(now).expect("delete grace"),
            now + Duration::seconds(spec.deletion_grace_period_seconds as i64)
        );

        let mut drifted = request;
        drifted.spec.maximum_sealed_recovery_points += 1;
        assert!(drifted.validate().is_err());
    }

    fn recovery_point(
        sequence: u64,
        predecessor_digest: Option<Sha256Digest>,
    ) -> DurableCellStorageRecoveryPointProjection {
        DurableCellStorageRecoveryPointProjection {
            namespace_id: StorageNamespaceId::new(),
            sequence,
            writer_epoch: 3,
            provider_profile_digest: Sha256Digest::from_bytes(b"profile"),
            manifest_key: format!("recovery/{sequence}/manifest"),
            manifest_digest: Sha256Digest::from_bytes(b"manifest"),
            state_digest: Sha256Digest::from_bytes(b"state"),
            state_size_bytes: 1,
            predecessor_digest,
            sealed_at: canonical_timestamp(Utc::now()),
            digest: Sha256Digest::from_bytes(format!("point-{sequence}").as_bytes()),
        }
    }

    #[test]
    fn seal_request_and_projection_retain_exact_lineage() {
        let request = DurableCellStorageSealRequest::new(
            OperationId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            StorageNamespaceId::new(),
            Sha256Digest::from_bytes(b"profile"),
            3,
            Sha256Digest::from_bytes(b"receipt"),
            canonical_timestamp(Utc::now()),
        );
        request.validate().expect("seal request");
        let mut input = DurableCellStorageSealInputProjection {
            operation_id: request.operation_id,
            organization_id: request.organization_id,
            project_id: request.project_id,
            environment_id: request.environment_id,
            namespace_id: request.namespace_id,
            provider_profile_digest: request.provider_profile_digest.clone(),
            writer_epoch: request.writer_epoch,
            writer_fence_receipt_digest: request.writer_fence_receipt_digest.clone(),
            sealed_at: request.sealed_at,
            previous_recovery_point: None,
        };
        input.validate_against(&request).expect("seal input");
        input.writer_epoch += 1;
        assert!(input.validate_against(&request).is_err());
    }

    #[test]
    fn recovery_projection_rejects_non_successor_sequences() {
        let first = recovery_point(1, None);
        let mut second = recovery_point(3, Some(first.digest.clone()));
        second.namespace_id = first.namespace_id;
        second.provider_profile_digest = first.provider_profile_digest.clone();
        assert!(second.validate_successor_of(&first).is_err());

        let mut successor = recovery_point(2, Some(first.digest.clone()));
        successor.namespace_id = first.namespace_id;
        successor.provider_profile_digest = first.provider_profile_digest.clone();
        successor.sealed_at = first.sealed_at + Duration::milliseconds(1);
        // The fixture digest is deliberately opaque; shape validation still
        // proves that a valid successor must carry canonical digests.
        assert!(successor.validate_successor_of(&first).is_ok());
    }
}
