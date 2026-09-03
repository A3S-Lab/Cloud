use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, EnvironmentId, OrganizationId, ProjectId, SecretVersionReference,
    Sha256Digest, StorageNamespaceId,
};
use async_trait::async_trait;
use serde::Serialize;

const MAX_CREDENTIAL_BINDING_BYTES: usize = 16 * 1024;

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
    async fn require_active_credentials(
        &self,
        request: &DurableCellStorageCredentialRequest,
    ) -> ApplicationResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::SecretId;

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
}
