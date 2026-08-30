use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, EnvironmentId, NodePoolId, OrganizationId, ProjectId,
    Sha256Digest, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, Workload, WorkloadControl, WorkloadRevision,
};
use a3s_cloud_contracts::{RuntimeIsolationLevel as IsolationLevel, RuntimeUnitClass};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DEPLOYMENT_RUNTIME_EXECUTION_BINDING_SCHEMA: &str =
    "a3s.cloud.deployment-runtime-execution-binding.v1";

/// Provider-neutral Runtime semantics admitted at the Workloads boundary.
///
/// Identity, or another future policy owner, may supply only these opaque
/// values. Workloads remains the sole compiler of artifact, process, network,
/// resource, mount, Secret, Unit, and generation state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadRuntimeExecutionBinding {
    runtime_class: RuntimeUnitClass,
    isolation: IsolationLevel,
    semantics_profile_digest: Sha256Digest,
    identity_attachment_digest: Sha256Digest,
}

impl WorkloadRuntimeExecutionBinding {
    pub fn new(
        runtime_class: RuntimeUnitClass,
        isolation: IsolationLevel,
        semantics_profile_digest: Sha256Digest,
        identity_attachment_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            runtime_class,
            isolation,
            semantics_profile_digest,
            identity_attachment_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Sha256Digest::parse(self.semantics_profile_digest.as_str())?
            != self.semantics_profile_digest
            || Sha256Digest::parse(self.identity_attachment_digest.as_str())?
                != self.identity_attachment_digest
        {
            return Err("Workload Runtime execution digest is not canonical".into());
        }
        Ok(())
    }

    pub const fn runtime_class(&self) -> RuntimeUnitClass {
        self.runtime_class
    }

    pub const fn isolation(&self) -> IsolationLevel {
        self.isolation
    }

    pub const fn semantics_profile_digest(&self) -> &Sha256Digest {
        &self.semantics_profile_digest
    }

    pub const fn identity_attachment_digest(&self) -> &Sha256Digest {
        &self.identity_attachment_digest
    }
}

/// Workloads-owned immutable admission fact for one exact Deployment.
///
/// This is intentionally not an Identity policy copy. It records only the
/// exact owner lineage, placement pool, generic Runtime semantics, source
/// authorization/admission time, including an explicit no-policy outcome, and
/// a deterministic digest needed to replay the same Runtime specification
/// after crashes or control-plane failover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentRuntimeExecutionBinding {
    schema: String,
    deployment_id: DeploymentId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    node_pool_id: Option<NodePoolId>,
    execution: Option<WorkloadRuntimeExecutionBinding>,
    authorized_at: Option<DateTime<Utc>>,
    admitted_at: DateTime<Utc>,
    binding_digest: Sha256Digest,
}

impl DeploymentRuntimeExecutionBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn bind(
        deployment: &Deployment,
        workload: &Workload,
        revision: &WorkloadRevision,
        control: &WorkloadControl,
        node_pool_id: NodePoolId,
        execution: WorkloadRuntimeExecutionBinding,
        authorized_at: DateTime<Utc>,
        admitted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: DEPLOYMENT_RUNTIME_EXECUTION_BINDING_SCHEMA.into(),
            deployment_id: deployment.id,
            organization_id: workload.organization_id,
            project_id: workload.project_id,
            environment_id: workload.environment_id,
            workload_id: workload.id,
            workload_revision_id: revision.id,
            node_pool_id: Some(node_pool_id),
            execution: Some(execution),
            authorized_at: Some(canonical_timestamp(authorized_at)),
            admitted_at: canonical_timestamp(admitted_at),
            binding_digest: Sha256Digest::from_bytes(&[]),
        };
        value.binding_digest = value.calculate_binding_digest()?;
        value.validate_admission(deployment, workload, revision, control)?;
        Ok(value)
    }

    pub fn admit_unbound(
        deployment: &Deployment,
        workload: &Workload,
        revision: &WorkloadRevision,
        control: &WorkloadControl,
        admitted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: DEPLOYMENT_RUNTIME_EXECUTION_BINDING_SCHEMA.into(),
            deployment_id: deployment.id,
            organization_id: workload.organization_id,
            project_id: workload.project_id,
            environment_id: workload.environment_id,
            workload_id: workload.id,
            workload_revision_id: revision.id,
            node_pool_id: control.spec.placement_policy.node_pool_id(),
            execution: None,
            authorized_at: None,
            admitted_at: canonical_timestamp(admitted_at),
            binding_digest: Sha256Digest::from_bytes(&[]),
        };
        value.binding_digest = value.calculate_binding_digest()?;
        value.validate_admission(deployment, workload, revision, control)?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        schema: String,
        deployment_id: DeploymentId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        node_pool_id: Option<NodePoolId>,
        execution: Option<WorkloadRuntimeExecutionBinding>,
        authorized_at: Option<DateTime<Utc>>,
        admitted_at: DateTime<Utc>,
        binding_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            schema,
            deployment_id,
            organization_id,
            project_id,
            environment_id,
            workload_id,
            workload_revision_id,
            node_pool_id,
            execution,
            authorized_at: authorized_at.map(canonical_timestamp),
            admitted_at: canonical_timestamp(admitted_at),
            binding_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(execution) = &self.execution {
            execution.validate()?;
        }
        let execution_shape_is_valid = matches!(
            (&self.execution, self.authorized_at, self.node_pool_id),
            (None, None, _) | (Some(_), Some(_), Some(_))
        );
        if self.schema != DEPLOYMENT_RUNTIME_EXECUTION_BINDING_SCHEMA
            || self.deployment_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self
                .node_pool_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .authorized_at
                .is_some_and(|value| value != canonical_timestamp(value))
            || self
                .authorized_at
                .is_some_and(|value| value > self.admitted_at)
            || self.admitted_at != canonical_timestamp(self.admitted_at)
            || !execution_shape_is_valid
            || Sha256Digest::parse(self.binding_digest.as_str())? != self.binding_digest
            || self.calculate_binding_digest()? != self.binding_digest
        {
            return Err("Deployment Runtime execution binding is invalid".into());
        }
        Ok(())
    }

    pub fn validate_lineage(
        &self,
        deployment: &Deployment,
        workload: &Workload,
        revision: &WorkloadRevision,
    ) -> Result<(), String> {
        self.validate()?;
        if self.deployment_id != deployment.id
            || self.organization_id != deployment.organization_id
            || self.organization_id != workload.organization_id
            || self.project_id != workload.project_id
            || self.environment_id != workload.environment_id
            || self.workload_id != deployment.workload_id
            || self.workload_id != workload.id
            || self.workload_revision_id != deployment.revision_id
            || self.workload_revision_id != revision.id
            || revision.workload_id != workload.id
            || self
                .execution
                .as_ref()
                .is_some_and(|execution| execution.runtime_class() != RuntimeUnitClass::Service)
        {
            return Err("Deployment Runtime execution binding changed owner lineage".into());
        }
        Ok(())
    }

    pub fn validate_admission(
        &self,
        deployment: &Deployment,
        workload: &Workload,
        revision: &WorkloadRevision,
        control: &WorkloadControl,
    ) -> Result<(), String> {
        self.validate_lineage(deployment, workload, revision)?;
        control.validate_against(workload)?;
        self.require_placement_lineage(deployment, control)?;
        if deployment.status != DeploymentStatus::Resolving
            || self.admitted_at < deployment.updated_at
            || deployment.node_id.is_some()
            || deployment.command_id.is_some()
            || deployment.cleanup_command_id.is_some()
            || deployment.retirement_command_id.is_some()
            || deployment.activated_at.is_some()
            || deployment.cancellation_requested_at.is_some()
            || deployment.cancelled_at.is_some()
        {
            return Err(
                "Deployment Runtime execution binding requires an unresolved Deployment".into(),
            );
        }
        Ok(())
    }

    /// Revalidates the only mutable part of admission lineage immediately
    /// before placement. Callers use this both as an early scheduling check
    /// and inside the repository transaction that commits node assignment.
    pub fn validate_placement_lineage(
        &self,
        deployment: &Deployment,
        control: &WorkloadControl,
    ) -> Result<(), String> {
        self.validate()?;
        control.spec.validate()?;
        self.require_placement_lineage(deployment, control)
    }

    fn require_placement_lineage(
        &self,
        deployment: &Deployment,
        control: &WorkloadControl,
    ) -> Result<(), String> {
        if self.deployment_id != deployment.id
            || self.organization_id != deployment.organization_id
            || self.workload_id != deployment.workload_id
            || self.workload_revision_id != deployment.revision_id
            || self.organization_id != control.organization_id
            || self.project_id != control.project_id
            || self.environment_id != control.environment_id
            || self.workload_id != control.workload_id
            || self.node_pool_id != control.spec.placement_policy.node_pool_id()
        {
            return Err("Deployment Runtime execution binding changed placement lineage".into());
        }
        Ok(())
    }

    fn calculate_binding_digest(&self) -> Result<Sha256Digest, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CanonicalBinding<'a> {
            schema: &'a str,
            deployment_id: DeploymentId,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
            workload_id: WorkloadId,
            workload_revision_id: WorkloadRevisionId,
            node_pool_id: Option<NodePoolId>,
            execution: Option<&'a WorkloadRuntimeExecutionBinding>,
            authorized_at: Option<DateTime<Utc>>,
            admitted_at: DateTime<Utc>,
        }

        let bytes = serde_json::to_vec(&CanonicalBinding {
            schema: &self.schema,
            deployment_id: self.deployment_id,
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            workload_id: self.workload_id,
            workload_revision_id: self.workload_revision_id,
            node_pool_id: self.node_pool_id,
            execution: self.execution.as_ref(),
            authorized_at: self.authorized_at,
            admitted_at: self.admitted_at,
        })
        .map_err(|error| format!("could not encode Deployment Runtime binding: {error}"))?;
        Ok(Sha256Digest::from_bytes(&bytes))
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub const fn deployment_id(&self) -> DeploymentId {
        self.deployment_id
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn workload_revision_id(&self) -> WorkloadRevisionId {
        self.workload_revision_id
    }

    pub const fn node_pool_id(&self) -> Option<NodePoolId> {
        self.node_pool_id
    }

    pub const fn execution(&self) -> Option<&WorkloadRuntimeExecutionBinding> {
        self.execution.as_ref()
    }

    pub const fn authorized_at(&self) -> Option<DateTime<Utc>> {
        self.authorized_at
    }

    pub const fn admitted_at(&self) -> DateTime<Utc> {
        self.admitted_at
    }

    pub const fn is_bound(&self) -> bool {
        self.execution.is_some()
    }

    pub const fn binding_digest(&self) -> &Sha256Digest {
        &self.binding_digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OperationId, ResourceName};
    use crate::modules::workloads::domain::entities::{
        OciArtifact, ServiceProcess, ServiceResources, ServiceTemplate, WorkloadControlSpec,
    };
    use chrono::Duration;
    use std::collections::BTreeMap;

    fn admission_fixture(
        node_pool_id: NodePoolId,
    ) -> (Deployment, Workload, WorkloadRevision, WorkloadControl) {
        let requested_at = canonical_timestamp(Utc::now());
        let workload = Workload::create(
            WorkloadId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("runtime-binding-fixture").expect("Workload name"),
            requested_at,
        );
        let digest = format!("sha256:{}", "a".repeat(64));
        let revision = WorkloadRevision::create(
            WorkloadRevisionId::new(),
            workload.id,
            1,
            ServiceTemplate {
                artifact: OciArtifact {
                    uri: format!("oci://registry.example/runtime-binding@{digest}"),
                    digest,
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ServiceProcess {
                    command: vec!["serve".into()],
                    args: Vec::new(),
                    working_directory: None,
                    environment: BTreeMap::new(),
                },
                secrets: Vec::new(),
                resources: ServiceResources {
                    cpu_millis: 100,
                    memory_bytes: 64 * 1024 * 1024,
                    pids: 32,
                    ephemeral_storage_bytes: None,
                },
                ports: Vec::new(),
                health: None,
            },
            requested_at,
        )
        .expect("Workload revision");
        let control = WorkloadControl::create(
            &workload,
            WorkloadControlSpec::unmanaged_replica_set_in_pool(1, 1, Some(node_pool_id))
                .expect("placement policy"),
        )
        .expect("Workload control");
        let mut deployment = Deployment::create(
            DeploymentId::new(),
            workload.organization_id,
            workload.id,
            revision.id,
            OperationId::new(),
            requested_at,
        );
        deployment
            .resolve(requested_at + Duration::milliseconds(1))
            .expect("resolve Deployment");
        (deployment, workload, revision, control)
    }

    #[test]
    fn execution_value_accepts_only_canonical_digests() {
        let value = WorkloadRuntimeExecutionBinding::new(
            RuntimeUnitClass::Service,
            IsolationLevel::Confidential,
            Sha256Digest::from_bytes(b"semantics"),
            Sha256Digest::from_bytes(b"identity"),
        )
        .expect("execution binding");

        value.validate().expect("valid execution binding");
        assert_eq!(value.runtime_class(), RuntimeUnitClass::Service);
        assert_eq!(value.isolation(), IsolationLevel::Confidential);
    }

    #[test]
    fn admission_is_pre_scheduling_and_fences_current_placement_lineage() {
        let node_pool_id = NodePoolId::new();
        let (deployment, workload, revision, control) = admission_fixture(node_pool_id);
        assert!(DeploymentRuntimeExecutionBinding::admit_unbound(
            &deployment,
            &workload,
            &revision,
            &control,
            deployment.requested_at,
        )
        .is_err());

        let binding = DeploymentRuntimeExecutionBinding::admit_unbound(
            &deployment,
            &workload,
            &revision,
            &control,
            deployment.updated_at,
        )
        .expect("unbound admission");
        binding
            .validate_placement_lineage(&deployment, &control)
            .expect("exact placement lineage");

        let changed_control = WorkloadControl::create(
            &workload,
            WorkloadControlSpec::unmanaged_replica_set_in_pool(1, 1, Some(NodePoolId::new()))
                .expect("changed placement policy"),
        )
        .expect("changed Workload control");
        assert!(binding
            .validate_placement_lineage(&deployment, &changed_control)
            .is_err());
    }
}
