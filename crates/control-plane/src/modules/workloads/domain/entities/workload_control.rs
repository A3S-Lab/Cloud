use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, ProjectId, WorkloadId,
};
use crate::modules::workloads::domain::entities::Workload;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const EFFECTIVE_PLACEMENT_POLICY_SCHEMA: &str = "a3s.cloud.effective-placement-policy.v1";
const MAX_OWNER_KIND_LENGTH: usize = 64;
pub const MAX_WORKLOAD_REPLICAS: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ManagedOwnerKind(String);

impl ManagedOwnerKind {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.len() > MAX_OWNER_KIND_LENGTH
            || value.split('.').count() < 2
            || value.split('.').any(|segment| {
                segment.is_empty()
                    || segment.len() > 32
                    || segment.bytes().enumerate().any(|(index, byte)| {
                        !(byte.is_ascii_lowercase()
                            || index > 0 && byte.is_ascii_digit()
                            || index > 0 && byte == b'-')
                    })
            })
        {
            return Err("managed owner kind must be a bounded dot-separated lowercase key".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedOwnerReference {
    kind: ManagedOwnerKind,
    owner_id: Uuid,
    owner_generation: u64,
    owner_spec_digest: String,
}

impl ManagedOwnerReference {
    pub fn new(
        kind: ManagedOwnerKind,
        owner_id: Uuid,
        owner_generation: u64,
        owner_spec_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let reference = Self {
            kind,
            owner_id,
            owner_generation,
            owner_spec_digest: owner_spec_digest.into(),
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.owner_id.is_nil()
            || self.owner_generation == 0
            || !is_sha256_digest(&self.owner_spec_digest)
        {
            return Err("managed owner reference is invalid".into());
        }
        ManagedOwnerKind::parse(self.kind.as_str().to_owned())?;
        Ok(())
    }

    pub fn kind(&self) -> &ManagedOwnerKind {
        &self.kind
    }

    pub const fn owner_id(&self) -> Uuid {
        self.owner_id
    }

    pub const fn owner_generation(&self) -> u64 {
        self.owner_generation
    }

    pub fn owner_spec_digest(&self) -> &str {
        &self.owner_spec_digest
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementTopology {
    SingleNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectivePlacementPolicy {
    schema: String,
    generation: u64,
    desired_replicas: u32,
    members_per_replica: u32,
    topology: PlacementTopology,
    digest: String,
}

impl EffectivePlacementPolicy {
    pub fn single_replica() -> Self {
        Self::replica_set(1, 1).expect("the built-in single-replica placement policy is valid")
    }

    pub fn replica_set(generation: u64, desired_replicas: u32) -> Result<Self, String> {
        let mut policy = Self {
            schema: EFFECTIVE_PLACEMENT_POLICY_SCHEMA.into(),
            generation,
            desired_replicas,
            members_per_replica: 1,
            topology: PlacementTopology::SingleNode,
            digest: String::new(),
        };
        policy.digest = policy.calculate_digest()?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EFFECTIVE_PLACEMENT_POLICY_SCHEMA
            || self.generation == 0
            || self.desired_replicas > MAX_WORKLOAD_REPLICAS
            || self.members_per_replica != 1
            || self.topology != PlacementTopology::SingleNode
            || !is_sha256_digest(&self.digest)
            || self.calculate_digest()? != self.digest
        {
            return Err(
                "effective placement policy is unsupported, corrupt, or not canonical".into(),
            );
        }
        Ok(())
    }

    pub fn document(&self) -> Result<serde_json::Value, String> {
        self.validate()?;
        serde_json::to_value(self)
            .map_err(|error| format!("could not encode effective placement policy: {error}"))
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn desired_replicas(&self) -> u32 {
        self.desired_replicas
    }

    pub const fn members_per_replica(&self) -> u32 {
        self.members_per_replica
    }

    pub const fn topology(&self) -> PlacementTopology {
        self.topology
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    fn calculate_digest(&self) -> Result<String, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DigestDocument<'a> {
            schema: &'a str,
            generation: u64,
            desired_replicas: u32,
            members_per_replica: u32,
            topology: PlacementTopology,
        }

        let encoded = serde_json::to_vec(&DigestDocument {
            schema: &self.schema,
            generation: self.generation,
            desired_replicas: self.desired_replicas,
            members_per_replica: self.members_per_replica,
            topology: self.topology,
        })
        .map_err(|error| format!("could not digest effective placement policy: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

impl Default for EffectivePlacementPolicy {
    fn default() -> Self {
        Self::single_replica()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadControlSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_owner: Option<ManagedOwnerReference>,
    pub placement_policy: EffectivePlacementPolicy,
}

impl WorkloadControlSpec {
    pub fn unmanaged_single_replica() -> Self {
        Self::unmanaged_replica_set(1, 1)
            .expect("the built-in unmanaged single-replica policy is valid")
    }

    pub fn unmanaged_replica_set(generation: u64, desired_replicas: u32) -> Result<Self, String> {
        Ok(Self {
            managed_owner: None,
            placement_policy: EffectivePlacementPolicy::replica_set(generation, desired_replicas)?,
        })
    }

    pub fn managed_single_replica(owner: ManagedOwnerReference) -> Result<Self, String> {
        Self::managed_replica_set(owner, 1, 1)
    }

    pub fn managed_replica_set(
        owner: ManagedOwnerReference,
        generation: u64,
        desired_replicas: u32,
    ) -> Result<Self, String> {
        owner.validate()?;
        Ok(Self {
            managed_owner: Some(owner),
            placement_policy: EffectivePlacementPolicy::replica_set(generation, desired_replicas)?,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(owner) = &self.managed_owner {
            owner.validate()?;
        }
        self.placement_policy.validate()
    }
}

impl Default for WorkloadControlSpec {
    fn default() -> Self {
        Self::unmanaged_single_replica()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadControl {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    pub workload_id: WorkloadId,
    pub spec: WorkloadControlSpec,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkloadControl {
    pub fn create(workload: &Workload, spec: WorkloadControlSpec) -> Result<Self, String> {
        spec.validate()?;
        let control = Self {
            organization_id: workload.organization_id,
            project_id: workload.project_id,
            environment_id: workload.environment_id,
            workload_id: workload.id,
            spec,
            aggregate_version: 1,
            created_at: workload.created_at,
            updated_at: workload.created_at,
        };
        control.validate_against(workload)?;
        Ok(control)
    }

    pub fn validate_against(&self, workload: &Workload) -> Result<(), String> {
        self.spec.validate()?;
        if self.organization_id != workload.organization_id
            || self.project_id != workload.project_id
            || self.environment_id != workload.environment_id
            || self.workload_id != workload.id
            || self.aggregate_version == 0
            || self.created_at != workload.created_at
            || self.updated_at < self.created_at
        {
            return Err("workload control does not match its Workload aggregate".into());
        }
        Ok(())
    }

    pub fn require_authority(&self, requested: &WorkloadControlSpec) -> Result<(), String> {
        requested.validate()?;
        if &self.spec != requested {
            return Err(
                "managed Workload mutation requires its exact immutable owner reference and effective placement policy"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn require_direct_mutation(&self) -> Result<(), String> {
        if self.spec.managed_owner.is_some() {
            return Err("managed Workload rejects direct mutation".into());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
        workload_id: WorkloadId,
        spec: WorkloadControlSpec,
        aggregate_version: u64,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        spec.validate()?;
        let created_at = canonical_timestamp(created_at);
        let updated_at = canonical_timestamp(updated_at);
        if aggregate_version == 0 || updated_at < created_at {
            return Err("stored workload control version or timestamps are invalid".into());
        }
        Ok(Self {
            organization_id,
            project_id,
            environment_id,
            workload_id,
            spec,
            aggregate_version,
            created_at,
            updated_at,
        })
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
