use super::{
    DurableCellApplication, DurableCellApplicationRevision, DurableCellProjectionIdentity,
    DurableCellStorageDeletionPlanIdentity, DurableCellStorageRecoveryPointIdentity,
    DurableCellStorageRestoreEvidenceIdentity, DurableCellStorageRestorePlanIdentity,
};
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

/// Provider-neutral input used to bind one exact S0 projection to a Durable
/// Cell revision. The application layer obtains these immutable identities
/// through the Storage port; Data credential and retention aggregates never
/// cross into the Durable Cells domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageBindingInput {
    pub namespace_id: StorageNamespaceId,
    pub credential_binding_generation: u64,
    pub credential_binding_digest: Sha256Digest,
    pub provider_profile_digest: Sha256Digest,
    pub retention_policy_digest: Sha256Digest,
}

impl DurableCellStorageBindingInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.namespace_id.as_uuid().is_nil()
            || self.credential_binding_generation == 0
            || Sha256Digest::parse(self.credential_binding_digest.as_str())?
                != self.credential_binding_digest
            || Sha256Digest::parse(self.provider_profile_digest.as_str())?
                != self.provider_profile_digest
            || Sha256Digest::parse(self.retention_policy_digest.as_str())?
                != self.retention_policy_digest
        {
            return Err("Durable Cell storage binding input is invalid".into());
        }
        Ok(())
    }
}

impl DurableCellStorageBinding {
    pub fn for_current_revision(
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        projection: &DurableCellProjectionIdentity,
        input: &DurableCellStorageBindingInput,
    ) -> Result<Self, String> {
        application.validate()?;
        revision.validate()?;
        projection
            .clone()
            .restore(application, revision)
            .map(drop)?;
        input.validate()?;
        if input.namespace_id != projection.storage_namespace_id {
            return Err("Durable Cell storage binding input has the wrong namespace".into());
        }
        let binding = Self {
            organization_id: application.organization_id,
            project_id: application.project_id,
            environment_id: application.environment_id,
            application_id: application.id,
            application_revision_id: revision.id,
            application_revision_number: revision.revision_number,
            application_definition_digest: revision.definition.digest().clone(),
            storage_namespace_id: projection.storage_namespace_id,
            credential_binding_generation: input.credential_binding_generation,
            credential_binding_digest: input.credential_binding_digest.clone(),
            provider_profile_digest: input.provider_profile_digest.clone(),
            retention_policy_digest: input.retention_policy_digest.clone(),
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn restore(
        self,
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        projection: &DurableCellProjectionIdentity,
        input: &DurableCellStorageBindingInput,
    ) -> Result<Self, String> {
        self.validate()?;
        let expected = Self::for_current_revision(application, revision, projection, input)?;
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

    pub fn validate_recovery_point(
        &self,
        point: &DurableCellStorageRecoveryPointIdentity,
        retention_policy_digest: &Sha256Digest,
    ) -> Result<(), String> {
        self.validate()?;
        point.validate()?;
        if Sha256Digest::parse(retention_policy_digest.as_str())? != *retention_policy_digest {
            return Err("Durable Cell retention identity is not canonical".into());
        }
        if point.namespace_id != self.storage_namespace_id
            || point.provider_profile_digest != self.provider_profile_digest
            || retention_policy_digest != &self.retention_policy_digest
        {
            return Err(
                "Durable Cell recovery point does not match the exact S0 storage binding".into(),
            );
        }
        Ok(())
    }

    pub fn validate_restore_plan(
        &self,
        point: &DurableCellStorageRecoveryPointIdentity,
        plan: &DurableCellStorageRestorePlanIdentity,
    ) -> Result<(), String> {
        self.validate_recovery_point(point, &plan.retention_policy_digest)?;
        plan.validate()?;
        if plan.source_namespace_id != self.storage_namespace_id
            || plan.source_provider_profile_digest != self.provider_profile_digest
            || plan.source_recovery_point_digest != point.digest
        {
            return Err("Durable Cell restore plan changed the bound S0 source".into());
        }
        Ok(())
    }

    pub fn validate_deletion_plan(
        &self,
        point: &DurableCellStorageRecoveryPointIdentity,
        restore_plan: &DurableCellStorageRestorePlanIdentity,
        restore_evidence: &DurableCellStorageRestoreEvidenceIdentity,
        deletion_plan: &DurableCellStorageDeletionPlanIdentity,
    ) -> Result<(), String> {
        self.validate_restore_plan(point, restore_plan)?;
        restore_evidence.validate()?;
        deletion_plan.validate()?;
        if restore_evidence.plan_digest != restore_plan.digest
            || restore_evidence.source_recovery_point_digest != point.digest
            || restore_evidence.target_namespace_id != restore_plan.target_namespace_id
            || deletion_plan.namespace_id != self.storage_namespace_id
            || deletion_plan.provider_profile_digest != self.provider_profile_digest
            || deletion_plan.retention_policy_digest != self.retention_policy_digest
            || deletion_plan.latest_recovery_point_digest != point.digest
            || deletion_plan.verified_restore_evidence_digest != restore_evidence.digest
            || deletion_plan.retained_restore_namespace_id != restore_plan.target_namespace_id
        {
            return Err("Durable Cell deletion plan changed the bound S0 namespace".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::data::{
        ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
        ObjectNamespaceDeletionPlan, ObjectNamespaceKey, ObjectNamespaceRecoveryPoint,
        ObjectNamespaceRecoveryPointSpec, ObjectNamespaceRestoreEvidence,
        ObjectNamespaceRestorePlan, ObjectNamespaceRetentionPolicy,
        ObjectNamespaceRetentionPolicySpec,
    };
    use crate::modules::durable_cells::domain::{
        DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
        DurableCellClassSpec, DurableCellRollbackPolicy, DurableCellStateSchema,
    };
    use crate::modules::shared_kernel::domain::{
        BuildRunId, PrincipalId, ResourceName, SecretId, SecretVersionReference,
    };
    use chrono::{Duration, Utc};

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

    fn retention_policy() -> ObjectNamespaceRetentionPolicy {
        ObjectNamespaceRetentionPolicy::from_spec(ObjectNamespaceRetentionPolicySpec {
            minimum_sealed_recovery_points: 1,
            maximum_sealed_recovery_points: 24,
            maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
            deletion_grace_period_seconds: 60 * 60,
        })
        .expect("retention policy")
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

    fn binding_input(
        credentials: &ObjectNamespaceCredentialBinding,
        retention_policy: &ObjectNamespaceRetentionPolicy,
    ) -> DurableCellStorageBindingInput {
        DurableCellStorageBindingInput {
            namespace_id: credentials.spec().namespace_id,
            credential_binding_generation: credentials.spec().generation,
            credential_binding_digest: credentials.digest().clone(),
            provider_profile_digest: credentials.spec().provider_profile_digest.clone(),
            retention_policy_digest: retention_policy.digest().clone(),
        }
    }

    fn recovery_point_identity(
        point: &ObjectNamespaceRecoveryPoint,
    ) -> DurableCellStorageRecoveryPointIdentity {
        DurableCellStorageRecoveryPointIdentity {
            namespace_id: point.spec().namespace_id,
            provider_profile_digest: point.spec().provider_profile_digest.clone(),
            digest: point.digest().clone(),
        }
    }

    fn restore_plan_identity(
        plan: &ObjectNamespaceRestorePlan,
    ) -> DurableCellStorageRestorePlanIdentity {
        DurableCellStorageRestorePlanIdentity {
            source_namespace_id: plan.spec().source_namespace_id,
            source_recovery_point_digest: plan.spec().source_recovery_point_digest.clone(),
            source_provider_profile_digest: plan.spec().source_provider_profile_digest.clone(),
            target_namespace_id: plan.spec().target_namespace_id,
            target_provider_profile_digest: plan.spec().target_provider_profile_digest.clone(),
            retention_policy_digest: plan.spec().retention_policy_digest.clone(),
            digest: plan.digest().clone(),
        }
    }

    fn restore_evidence_identity(
        evidence: &ObjectNamespaceRestoreEvidence,
    ) -> DurableCellStorageRestoreEvidenceIdentity {
        DurableCellStorageRestoreEvidenceIdentity {
            plan_digest: evidence.plan_digest.clone(),
            source_recovery_point_digest: evidence.source_recovery_point_digest.clone(),
            target_namespace_id: evidence.target_namespace_id,
            digest: evidence.digest().clone(),
        }
    }

    fn deletion_plan_identity(
        plan: &ObjectNamespaceDeletionPlan,
    ) -> DurableCellStorageDeletionPlanIdentity {
        DurableCellStorageDeletionPlanIdentity {
            namespace_id: plan.spec().namespace_id,
            provider_profile_digest: plan.spec().provider_profile_digest.clone(),
            retention_policy_digest: plan.spec().retention_policy_digest.clone(),
            latest_recovery_point_digest: plan.spec().latest_recovery_point_digest.clone(),
            verified_restore_evidence_digest: plan.spec().verified_restore_evidence_digest.clone(),
            retained_restore_namespace_id: plan.spec().retained_restore_namespace_id,
            digest: plan.digest().clone(),
        }
    }

    #[test]
    fn binding_requires_exact_current_revision_namespace_and_credential_scope() {
        let (application, revision, projection, credentials) = fixture();
        let retention_policy = retention_policy();
        let input = binding_input(&credentials, &retention_policy);
        let binding = DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &input,
        )
        .expect("storage binding");
        assert_eq!(
            binding.storage_namespace_id,
            projection.storage_namespace_id
        );
        assert_eq!(binding.credential_binding_generation, 1);
        binding
            .clone()
            .restore(&application, &revision, &projection, &input)
            .expect("restore");

        let mut foreign = credentials.spec().clone();
        foreign.namespace_id = StorageNamespaceId::new();
        let foreign = ObjectNamespaceCredentialBinding::from_spec(foreign).expect("foreign");
        let foreign_input = binding_input(&foreign, &retention_policy);
        assert!(DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &foreign_input
        )
        .is_err());
    }

    #[test]
    fn credential_rotation_changes_only_the_exact_s0_binding() {
        let (application, revision, projection, credentials) = fixture();
        let retention_policy = retention_policy();
        let input = binding_input(&credentials, &retention_policy);
        let initial = DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &input,
        )
        .expect("initial");
        let mut rotated = credentials.spec().clone();
        rotated.generation = 2;
        rotated.secret_access_key = reference();
        let rotated = ObjectNamespaceCredentialBinding::from_spec(rotated).expect("rotated");
        rotated
            .validate_successor_of(&credentials)
            .expect("credential lineage");
        let rotated_input = binding_input(&rotated, &retention_policy);
        let successor = DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &rotated_input,
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

    #[test]
    fn recovery_and_deletion_remain_exact_s0_contracts() {
        let (application, revision, projection, credentials) = fixture();
        let retention_policy = retention_policy();
        let input = binding_input(&credentials, &retention_policy);
        let binding = DurableCellStorageBinding::for_current_revision(
            &application,
            &revision,
            &projection,
            &input,
        )
        .expect("storage binding");
        let now = Utc::now();
        let point = ObjectNamespaceRecoveryPoint::seal(ObjectNamespaceRecoveryPointSpec {
            namespace_id: binding.storage_namespace_id,
            sequence: 1,
            writer_epoch: 4,
            provider_profile_digest: binding.provider_profile_digest.clone(),
            manifest_key: ObjectNamespaceKey::parse("recovery/epoch-4/manifest")
                .expect("manifest key"),
            manifest_digest: digest('e'),
            state_digest: digest('f'),
            state_size_bytes: 4096,
            predecessor_digest: None,
            sealed_at: now,
        })
        .expect("recovery point");
        let point_identity = recovery_point_identity(&point);
        let retention_policy_digest = retention_policy.digest().clone();
        binding
            .validate_recovery_point(&point_identity, &retention_policy_digest)
            .expect("bound recovery point");
        let restore_plan = ObjectNamespaceRestorePlan::for_recovery_point(
            &point,
            StorageNamespaceId::new(),
            digest('1'),
            &retention_policy,
            now + Duration::seconds(1),
        )
        .expect("restore plan");
        let restore_plan_identity = restore_plan_identity(&restore_plan);
        binding
            .validate_restore_plan(&point_identity, &restore_plan_identity)
            .expect("bound restore");
        let restore_evidence = ObjectNamespaceRestoreEvidence::verified(
            &restore_plan,
            digest('2'),
            now + Duration::seconds(2),
        )
        .expect("restore evidence");
        let restore_evidence_identity = restore_evidence_identity(&restore_evidence);
        let deletion_plan = ObjectNamespaceDeletionPlan::after_verified_restore(
            &point,
            &restore_plan,
            &restore_evidence,
            &retention_policy,
            digest('3'),
            digest('4'),
            now + Duration::seconds(3),
        )
        .expect("deletion plan");
        let deletion_plan_identity = deletion_plan_identity(&deletion_plan);
        binding
            .validate_deletion_plan(
                &point_identity,
                &restore_plan_identity,
                &restore_evidence_identity,
                &deletion_plan_identity,
            )
            .expect("bound deletion");

        let foreign_point = ObjectNamespaceRecoveryPoint::seal(ObjectNamespaceRecoveryPointSpec {
            namespace_id: StorageNamespaceId::new(),
            ..point.spec().clone()
        })
        .expect("foreign point");
        let foreign_point_identity = recovery_point_identity(&foreign_point);
        assert!(binding
            .validate_recovery_point(&foreign_point_identity, &retention_policy_digest)
            .is_err());

        let mut drifted_restore_plan = restore_plan_identity.clone();
        drifted_restore_plan.source_recovery_point_digest = digest('9');
        assert!(binding
            .validate_restore_plan(&point_identity, &drifted_restore_plan)
            .is_err());

        let mut drifted_restore_evidence = restore_evidence_identity.clone();
        drifted_restore_evidence.plan_digest = digest('8');
        assert!(binding
            .validate_deletion_plan(
                &point_identity,
                &restore_plan_identity,
                &drifted_restore_evidence,
                &deletion_plan_identity,
            )
            .is_err());

        let mut drifted_deletion_plan = deletion_plan_identity.clone();
        drifted_deletion_plan.retained_restore_namespace_id = StorageNamespaceId::new();
        assert!(binding
            .validate_deletion_plan(
                &point_identity,
                &restore_plan_identity,
                &restore_evidence_identity,
                &drifted_deletion_plan,
            )
            .is_err());
    }
}
