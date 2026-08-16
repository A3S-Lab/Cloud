use super::{DurableCellApplication, DurableCellApplicationRevision};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    OperationId, OrganizationId, ProjectId, Sha256Digest, StorageNamespaceId, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::{ManagedOwnerKind, ManagedOwnerReference};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DURABLE_CELL_MANAGED_OWNER_KIND: &str = "durable-cell.application";

const STORAGE_NAMESPACE_ID_NAME: &[u8] = b"a3s-cloud:durable-cell:storage-namespace:v1";
const WORKLOAD_ID_NAME: &[u8] = b"a3s-cloud:durable-cell:workload:v1";
const WORKLOAD_REVISION_ID_NAME: &[u8] = b"a3s-cloud:durable-cell:workload-revision:v1";
const DEPLOYMENT_ID_NAME: &[u8] = b"a3s-cloud:durable-cell:workload-deployment:v1";
const OPERATION_ID_NAME: &[u8] = b"a3s-cloud:durable-cell:deployment-operation:v1";

/// Stable identities for projecting one current Durable Cell application
/// revision through existing platform owners.
///
/// This is not a deployment aggregate or lifecycle. Workloads owns the
/// Workload revision and Deployment, Operations owns the operation, and S0
/// will own the storage namespace. Gateway scope selection is deliberately
/// absent because it is an environment-owned input to later orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellProjectionIdentity {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub application_revision_number: u64,
    pub application_definition_digest: Sha256Digest,
    pub storage_namespace_id: StorageNamespaceId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
}

impl DurableCellProjectionIdentity {
    pub fn storage_namespace_id_for_application(
        application_id: DurableCellApplicationId,
    ) -> StorageNamespaceId {
        StorageNamespaceId::from_uuid(derived_id(
            application_id.as_uuid(),
            STORAGE_NAMESPACE_ID_NAME,
        ))
    }

    pub fn workload_revision_id_for_application_revision(
        revision_id: DurableCellApplicationRevisionId,
    ) -> WorkloadRevisionId {
        WorkloadRevisionId::from_uuid(derived_id(revision_id.as_uuid(), WORKLOAD_REVISION_ID_NAME))
    }

    pub fn for_current_revision(
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
    ) -> Result<Self, String> {
        application.validate()?;
        revision.validate()?;
        if revision.organization_id != application.organization_id
            || revision.project_id != application.project_id
            || revision.environment_id != application.environment_id
            || revision.application_id != application.id
            || revision.id != application.current_revision_id
            || revision.revision_number != application.current_revision_number
            || revision.definition.digest() != &application.current_definition_digest
        {
            return Err(
                "Durable Cell projection requires the application's exact current revision".into(),
            );
        }
        let identity = Self {
            organization_id: application.organization_id,
            project_id: application.project_id,
            environment_id: application.environment_id,
            application_id: application.id,
            application_revision_id: revision.id,
            application_revision_number: revision.revision_number,
            application_definition_digest: revision.definition.digest().clone(),
            storage_namespace_id: Self::storage_namespace_id_for_application(application.id),
            workload_id: WorkloadId::from_uuid(derived_id(
                application.id.as_uuid(),
                WORKLOAD_ID_NAME,
            )),
            workload_revision_id: Self::workload_revision_id_for_application_revision(revision.id),
            deployment_id: DeploymentId::from_uuid(derived_id(
                revision.id.as_uuid(),
                DEPLOYMENT_ID_NAME,
            )),
            operation_id: OperationId::from_uuid(derived_id(
                revision.id.as_uuid(),
                OPERATION_ID_NAME,
            )),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn restore(
        self,
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
    ) -> Result<Self, String> {
        self.validate()?;
        let expected = Self::for_current_revision(application, revision)?;
        if self != expected {
            return Err("stored Durable Cell projection identity drifted".into());
        }
        Ok(self)
    }

    pub fn managed_owner_reference(&self) -> Result<ManagedOwnerReference, String> {
        self.validate()?;
        ManagedOwnerReference::new(
            ManagedOwnerKind::parse(DURABLE_CELL_MANAGED_OWNER_KIND)?,
            self.application_id.as_uuid(),
            self.application_revision_number,
            self.application_definition_digest.as_str(),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_revision_id.as_uuid().is_nil()
            || self.application_revision_number == 0
            || Sha256Digest::parse(self.application_definition_digest.as_str())?
                != self.application_definition_digest
            || self.storage_namespace_id
                != Self::storage_namespace_id_for_application(self.application_id)
            || self.workload_id.as_uuid()
                != derived_id(self.application_id.as_uuid(), WORKLOAD_ID_NAME)
            || self.workload_revision_id
                != Self::workload_revision_id_for_application_revision(self.application_revision_id)
            || self.deployment_id.as_uuid()
                != derived_id(self.application_revision_id.as_uuid(), DEPLOYMENT_ID_NAME)
            || self.operation_id.as_uuid()
                != derived_id(self.application_revision_id.as_uuid(), OPERATION_ID_NAME)
        {
            return Err("Durable Cell projection identity is invalid".into());
        }
        Ok(())
    }
}

fn derived_id(parent: Uuid, name: &[u8]) -> Uuid {
    Uuid::new_v5(&parent, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::durable_cells::domain::{
        DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
        DurableCellApplicationDesiredState, DurableCellClassSpec, DurableCellRollbackPolicy,
        DurableCellStateSchema,
    };
    use crate::modules::shared_kernel::domain::{BuildRunId, PrincipalId, ResourceName};
    use chrono::Utc;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn definition(character: char, write_version: u64) -> DurableCellApplicationDefinition {
        DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
            build_run_id: BuildRunId::new(),
            bundle_digest: digest(character),
            bundle_size_bytes: 1024,
            main_module: "worker.mjs".into(),
            compatibility_date: "2026-08-15".into(),
            compatibility_flags: Vec::new(),
            cell_classes: vec![DurableCellClassSpec {
                name: "Counter".into(),
                state_schema: DurableCellStateSchema {
                    minimum_readable_version: 1,
                    maximum_readable_version: 2,
                    write_version,
                },
            }],
            service_profile_digest: digest('f'),
            rollback_policy: DurableCellRollbackPolicy::Compatible,
        })
        .expect("definition")
    }

    fn fixture_application() -> (DurableCellApplication, DurableCellApplicationRevision) {
        let application_id = DurableCellApplicationId::new();
        let revision = DurableCellApplicationRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition('a', 1),
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
        (application, revision)
    }

    #[test]
    fn projection_reuses_managed_owner_and_stable_platform_identities() {
        let (application, initial) = fixture_application();
        let initial_identity =
            DurableCellProjectionIdentity::for_current_revision(&application, &initial)
                .expect("initial identity");
        let owner = initial_identity
            .managed_owner_reference()
            .expect("managed owner");
        assert_eq!(owner.kind().as_str(), DURABLE_CELL_MANAGED_OWNER_KIND);
        assert_eq!(owner.owner_id(), application.id.as_uuid());
        assert_eq!(owner.owner_generation(), 1);
        assert_eq!(
            owner.owner_spec_digest(),
            initial.definition.digest().as_str()
        );

        let successor = DurableCellApplicationRevision::successor(
            &initial,
            DurableCellApplicationRevisionId::new(),
            definition('b', 2),
            PrincipalId::new(),
            initial.created_at,
        )
        .expect("successor");
        let application = application.advance(1, &successor).expect("advance");
        let successor_identity =
            DurableCellProjectionIdentity::for_current_revision(&application, &successor)
                .expect("successor identity");
        assert_eq!(
            successor_identity.storage_namespace_id,
            initial_identity.storage_namespace_id
        );
        assert_eq!(successor_identity.workload_id, initial_identity.workload_id);
        assert_ne!(
            successor_identity.workload_revision_id,
            initial_identity.workload_revision_id
        );
        assert_ne!(
            successor_identity.deployment_id,
            initial_identity.deployment_id
        );
        assert_ne!(
            successor_identity.operation_id,
            initial_identity.operation_id
        );
    }

    #[test]
    fn projection_rejects_stale_foreign_or_drifted_identity() {
        let (application, revision) = fixture_application();
        let mut identity =
            DurableCellProjectionIdentity::for_current_revision(&application, &revision)
                .expect("identity");
        identity.workload_id = WorkloadId::new();
        assert!(identity.validate().is_err());
        assert!(identity.restore(&application, &revision).is_err());

        let stopped = application
            .request_state(
                application.aggregate_version,
                DurableCellApplicationDesiredState::Stopped,
                revision.created_at,
            )
            .expect("stop");
        DurableCellProjectionIdentity::for_current_revision(&stopped, &revision)
            .expect("identity is independent of desired execution state");

        let (_, foreign_revision) = fixture_application();
        assert!(DurableCellProjectionIdentity::for_current_revision(
            &application,
            &foreign_revision
        )
        .is_err());
    }
}
