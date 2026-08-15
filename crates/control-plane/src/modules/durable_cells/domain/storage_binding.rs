use super::{
    DurableCellApplication, DurableCellApplicationRevision, DurableCellProjectionIdentity,
};
use crate::modules::data::ObjectNamespaceCredentialBinding;
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, OrganizationId,
    ProjectId, Sha256Digest, StorageNamespaceId,
};
use serde::{Deserialize, Serialize};

/// Plaintext-free correlation from one exact Durable Cell revision to the
/// S0-owned namespace and credential generation it may consume.
///
/// This value owns no credential material, provider configuration, namespace
/// lifecycle, object client, backup process, or deployment state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellStorageBinding {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub application_revision_number: u64,
    pub application_definition_digest: Sha256Digest,
    pub storage_namespace_id: StorageNamespaceId,
    pub credential_binding_generation: u64,
    pub credential_binding_digest: Sha256Digest,
    pub provider_profile_digest: Sha256Digest,
    pub retention_policy_digest: Sha256Digest,
}

impl DurableCellStorageBinding {
    pub fn for_current_revision(
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        projection: &DurableCellProjectionIdentity,
        credentials: &ObjectNamespaceCredentialBinding,
        retention_policy_digest: Sha256Digest,
    ) -> Result<Self, String> {
        application.validate()?;
        revision.validate()?;
        projection
            .clone()
            .restore(application, revision)
            .map(drop)?;
        credentials.validate_scope(
            application.organization_id,
            application.project_id,
            application.environment_id,
            projection.storage_namespace_id,
        )?;
        Sha256Digest::parse(retention_policy_digest.as_str())?;
        let binding = Self {
            organization_id: application.organization_id,
            project_id: application.project_id,
            environment_id: application.environment_id,
            application_id: application.id,
            application_revision_id: revision.id,
            application_revision_number: revision.revision_number,
            application_definition_digest: revision.definition.digest().clone(),
            storage_namespace_id: projection.storage_namespace_id,
            credential_binding_generation: credentials.spec().generation,
            credential_binding_digest: credentials.digest().clone(),
            provider_profile_digest: credentials.spec().provider_profile_digest.clone(),
            retention_policy_digest,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn restore(
        self,
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        projection: &DurableCellProjectionIdentity,
        credentials: &ObjectNamespaceCredentialBinding,
        retention_policy_digest: Sha256Digest,
    ) -> Result<Self, String> {
        self.validate()?;
        let expected = Self::for_current_revision(
            application,
            revision,
            projection,
            credentials,
            retention_policy_digest,
        )?;
        if self != expected {
            return Err("stored Durable Cell storage binding drifted".into());
        }
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_revision_id.as_uuid().is_nil()
            || self.application_revision_number == 0
            || self.storage_namespace_id.as_uuid().is_nil()
            || self.credential_binding_generation == 0
            || Sha256Digest::parse(self.application_definition_digest.as_str())?
                != self.application_definition_digest
            || Sha256Digest::parse(self.credential_binding_digest.as_str())?
                != self.credential_binding_digest
            || Sha256Digest::parse(self.provider_profile_digest.as_str())?
                != self.provider_profile_digest
            || Sha256Digest::parse(self.retention_policy_digest.as_str())?
                != self.retention_policy_digest
        {
            return Err("Durable Cell storage binding is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::data::ObjectNamespaceCredentialBindingSpec;
    use crate::modules::durable_cells::domain::{
        DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
        DurableCellClassSpec, DurableCellRollbackPolicy, DurableCellStateSchema,
    };
    use crate::modules::shared_kernel::domain::{
        BuildRunId, PrincipalId, ResourceName, SecretId, SecretVersionReference,
    };
    use chrono::Utc;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn definition() -> DurableCellApplicationDefinition {
        DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
            build_run_id: BuildRunId::new(),
            bundle_digest: digest('a'),
            bundle_size_bytes: 1024,
            main_module: "worker.mjs".into(),
            compatibility_date: "2026-08-16".into(),
            compatibility_flags: Vec::new(),
            cell_classes: vec![DurableCellClassSpec {
                name: "Counter".into(),
                state_schema: DurableCellStateSchema {
                    minimum_readable_version: 1,
                    maximum_readable_version: 1,
                    write_version: 1,
                },
            }],
            service_profile_digest: digest('b'),
            rollback_policy: DurableCellRollbackPolicy::Compatible,
        })
        .expect("definition")
    }

    fn reference() -> SecretVersionReference {
        SecretVersionReference::new(SecretId::new(), 1).expect("reference")
    }

    fn fixture() -> (
        DurableCellApplication,
        DurableCellApplicationRevision,
        DurableCellProjectionIdentity,
        ObjectNamespaceCredentialBinding,
    ) {
        let application_id = DurableCellApplicationId::new();
        let revision = DurableCellApplicationRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition(),
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("revision");
        let application = DurableCellApplication::create(
            application_id,
            ResourceName::parse("Counters").expect("name"),
            &revision,
        )
        .expect("application");
        let projection =
            DurableCellProjectionIdentity::for_current_revision(&application, &revision)
                .expect("projection");
        let credentials =
            ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
                organization_id: application.organization_id,
                project_id: application.project_id,
                environment_id: application.environment_id,
                namespace_id: projection.storage_namespace_id,
                generation: 1,
                provider_profile_digest: digest('c'),
                access_key_id: reference(),
                secret_access_key: reference(),
                session_token: None,
            })
            .expect("credentials");
        (application, revision, projection, credentials)
    }

    #[test]
    fn binding_requires_exact_current_revision_namespace_and_credential_scope() {
        let (application, revision, projection, credentials) = fixture();
        let binding = DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &credentials,
            digest('d'),
        )
        .expect("storage binding");
        assert_eq!(
            binding.storage_namespace_id,
            projection.storage_namespace_id
        );
        assert_eq!(binding.credential_binding_generation, 1);
        binding
            .clone()
            .restore(
                &application,
                &revision,
                &projection,
                &credentials,
                digest('d'),
            )
            .expect("restore");

        let mut foreign = credentials.spec().clone();
        foreign.namespace_id = StorageNamespaceId::new();
        let foreign = ObjectNamespaceCredentialBinding::from_spec(foreign).expect("foreign");
        assert!(DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &foreign,
            digest('d')
        )
        .is_err());
    }

    #[test]
    fn credential_rotation_changes_only_the_exact_s0_binding() {
        let (application, revision, projection, credentials) = fixture();
        let initial = DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &credentials,
            digest('d'),
        )
        .expect("initial");
        let mut rotated = credentials.spec().clone();
        rotated.generation = 2;
        rotated.secret_access_key = reference();
        let rotated = ObjectNamespaceCredentialBinding::from_spec(rotated).expect("rotated");
        rotated
            .validate_successor_of(&credentials)
            .expect("credential lineage");
        let successor = DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &rotated,
            digest('d'),
        )
        .expect("successor");
        assert_eq!(
            successor.application_revision_id,
            initial.application_revision_id
        );
        assert_eq!(successor.storage_namespace_id, initial.storage_namespace_id);
        assert_eq!(successor.credential_binding_generation, 2);
        assert_ne!(
            successor.credential_binding_digest,
            initial.credential_binding_digest
        );
    }
}
