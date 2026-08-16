use super::ObjectNamespaceProviderProfile;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, EnvironmentId, OrganizationId, ProjectId, SecretVersionReference,
    Sha256Digest, StorageNamespaceId,
};
use serde::{Deserialize, Serialize};

const MAX_CREDENTIAL_BINDING_BYTES: usize = 16 * 1024;

/// Exact, plaintext-free credential projection for one S0 object namespace.
///
/// The provider profile owns endpoint/bucket semantics. This binding owns only
/// tenant scope, namespace scope, immutable profile identity, and exact Secret
/// versions. Secrets remains the sole materialization authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceCredentialBindingSpec {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub namespace_id: StorageNamespaceId,
    pub generation: u64,
    pub provider_profile_digest: Sha256Digest,
    pub access_key_id: SecretVersionReference,
    pub secret_access_key: SecretVersionReference,
    pub session_token: Option<SecretVersionReference>,
}

impl ObjectNamespaceCredentialBindingSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.namespace_id.as_uuid().is_nil()
            || self.generation == 0
            || Sha256Digest::parse(self.provider_profile_digest.as_str())?
                != self.provider_profile_digest
        {
            return Err("object namespace credential binding scope is invalid".into());
        }
        self.access_key_id.validate()?;
        self.secret_access_key.validate()?;
        if let Some(session_token) = self.session_token {
            session_token.validate()?;
        }
        let mut identities = self
            .references()
            .into_iter()
            .map(|reference| reference.secret_id)
            .collect::<Vec<_>>();
        identities.sort_unstable();
        identities.dedup();
        if identities.len() != self.references().len() {
            return Err(
                "object namespace credential fields require distinct Secret identities".into(),
            );
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceCredentialBinding {
    spec: ObjectNamespaceCredentialBindingSpec,
    digest: Sha256Digest,
}

impl ObjectNamespaceCredentialBinding {
    pub fn from_spec(spec: ObjectNamespaceCredentialBindingSpec) -> Result<Self, String> {
        spec.validate()?;
        let digest = binding_digest(&spec)?;
        Ok(Self { spec, digest })
    }

    pub fn restore(
        spec: ObjectNamespaceCredentialBindingSpec,
        stored_digest: &str,
    ) -> Result<Self, String> {
        let binding = Self::from_spec(spec)?;
        if binding.digest.as_str() != stored_digest {
            return Err("stored object namespace credential binding digest drifted".into());
        }
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if &Self::restore(self.spec.clone(), self.digest.as_str())? != self {
            return Err("object namespace credential binding drifted".into());
        }
        Ok(())
    }

    pub fn validate_scope(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        namespace_id: StorageNamespaceId,
    ) -> Result<(), String> {
        self.validate()?;
        if self.spec.organization_id != organization_id
            || self.spec.project_id != project_id
            || self.spec.environment_id != environment_id
            || self.spec.namespace_id != namespace_id
        {
            return Err("object namespace credential binding has the wrong exact scope".into());
        }
        Ok(())
    }

    pub fn validate_successor_of(&self, previous: &Self) -> Result<(), String> {
        self.validate()?;
        previous.validate()?;
        if self.spec.organization_id != previous.spec.organization_id
            || self.spec.project_id != previous.spec.project_id
            || self.spec.environment_id != previous.spec.environment_id
            || self.spec.namespace_id != previous.spec.namespace_id
            || self.spec.generation
                != previous.spec.generation.checked_add(1).ok_or_else(|| {
                    "object namespace credential generation is exhausted".to_owned()
                })?
            || self.digest == previous.digest
        {
            return Err(
                "object namespace credential successor must preserve scope and advance exactly once"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn validate_provider_profile(
        &self,
        profile: &ObjectNamespaceProviderProfile,
    ) -> Result<(), String> {
        self.validate()?;
        profile.validate()?;
        if &self.spec.provider_profile_digest != profile.digest() {
            return Err(
                "object namespace credential binding and provider profile digests differ".into(),
            );
        }
        Ok(())
    }

    pub fn spec(&self) -> &ObjectNamespaceCredentialBindingSpec {
        &self.spec
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn binding_digest(spec: &ObjectNamespaceCredentialBindingSpec) -> Result<Sha256Digest, String> {
    let bytes = canonical_json_bounded(
        spec,
        MAX_CREDENTIAL_BINDING_BYTES,
        "object namespace credential binding",
    )?;
    Ok(Sha256Digest::from_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::SecretId;

    fn reference() -> SecretVersionReference {
        SecretVersionReference::new(SecretId::new(), 1).expect("reference")
    }

    fn spec() -> ObjectNamespaceCredentialBindingSpec {
        ObjectNamespaceCredentialBindingSpec {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            namespace_id: StorageNamespaceId::new(),
            generation: 1,
            provider_profile_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                .expect("digest"),
            access_key_id: reference(),
            secret_access_key: reference(),
            session_token: Some(reference()),
        }
    }

    #[test]
    fn binding_is_digest_locked_and_exactly_scoped() {
        let spec = spec();
        let binding = ObjectNamespaceCredentialBinding::from_spec(spec.clone()).expect("binding");
        binding
            .validate_scope(
                spec.organization_id,
                spec.project_id,
                spec.environment_id,
                spec.namespace_id,
            )
            .expect("scope");
        assert!(binding
            .validate_scope(
                spec.organization_id,
                ProjectId::new(),
                spec.environment_id,
                spec.namespace_id,
            )
            .is_err());
        assert!(ObjectNamespaceCredentialBinding::restore(
            spec,
            &format!("sha256:{}", "b".repeat(64))
        )
        .is_err());
    }

    #[test]
    fn binding_rejects_aliased_fields_and_requires_exact_successor_generation() {
        let previous = ObjectNamespaceCredentialBinding::from_spec(spec()).expect("previous");
        let mut aliased = previous.spec().clone();
        aliased.secret_access_key =
            SecretVersionReference::new(aliased.access_key_id.secret_id, 2).expect("alias");
        assert!(ObjectNamespaceCredentialBinding::from_spec(aliased).is_err());

        let mut next = previous.spec().clone();
        next.generation = 2;
        next.secret_access_key = reference();
        let next = ObjectNamespaceCredentialBinding::from_spec(next).expect("successor");
        next.validate_successor_of(&previous).expect("lineage");

        let mut skipped = next.spec().clone();
        skipped.generation = 4;
        skipped.access_key_id = reference();
        let skipped = ObjectNamespaceCredentialBinding::from_spec(skipped).expect("binding");
        assert!(skipped.validate_successor_of(&next).is_err());
    }

    #[test]
    fn binding_requires_the_exact_non_secret_provider_profile() {
        let profile = ObjectNamespaceProviderProfile::from_spec(
            super::super::ObjectNamespaceProviderProfileSpec {
                endpoint: "https://s3.example.com".into(),
                region: "us-east-1".into(),
                bucket: "a3s-durable-cells".into(),
                prefix: "a3s/durable-cells".into(),
                virtual_hosted_style: false,
            },
        )
        .expect("profile");
        let mut exact = spec();
        exact.provider_profile_digest = profile.digest().clone();
        ObjectNamespaceCredentialBinding::from_spec(exact)
            .expect("binding")
            .validate_provider_profile(&profile)
            .expect("exact provider profile");

        assert!(ObjectNamespaceCredentialBinding::from_spec(spec())
            .expect("foreign binding")
            .validate_provider_profile(&profile)
            .is_err());
    }
}
