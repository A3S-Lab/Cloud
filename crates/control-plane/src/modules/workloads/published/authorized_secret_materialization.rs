use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, SecretId, WorkloadId, WorkloadRevisionId,
};
use serde::{Deserialize, Serialize};

pub const AUTHORIZED_WORKLOAD_SECRET_MATERIALIZATION_SCHEMA: &str =
    "a3s.cloud.authorized-workload-secret-materialization.v1";

/// Workloads-owned immutable proof that one exact node may materialize one
/// exact Secret version bound by one exact Workload revision.
///
/// Deployment state, placement records, and Workload aggregates remain inside
/// Workloads. Consumers receive only the minimum owner evidence required to
/// enforce their own boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizedWorkloadSecretMaterialization {
    schema: String,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    node_id: NodeId,
    secret_id: SecretId,
    secret_version: u64,
}

pub(in crate::modules::workloads) struct ValidatedSecretMaterializationProjection {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub secret_id: SecretId,
    pub secret_version: u64,
}

impl AuthorizedWorkloadSecretMaterialization {
    pub(in crate::modules::workloads) fn from_validated_workload(
        projection: ValidatedSecretMaterializationProjection,
    ) -> Result<Self, String> {
        let value = Self {
            schema: AUTHORIZED_WORKLOAD_SECRET_MATERIALIZATION_SCHEMA.into(),
            organization_id: projection.organization_id,
            project_id: projection.project_id,
            environment_id: projection.environment_id,
            workload_id: projection.workload_id,
            workload_revision_id: projection.workload_revision_id,
            node_id: projection.node_id,
            secret_id: projection.secret_id,
            secret_version: projection.secret_version,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORIZED_WORKLOAD_SECRET_MATERIALIZATION_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.secret_id.as_uuid().is_nil()
            || self.secret_version == 0
        {
            return Err("authorized Workload Secret materialization is invalid".into());
        }
        Ok(())
    }

    pub fn schema(&self) -> &str {
        &self.schema
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

    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub const fn secret_id(&self) -> SecretId {
        self.secret_id
    }

    pub const fn secret_version(&self) -> u64 {
        self.secret_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_fact_rejects_identity_or_version_drift() {
        let fact = AuthorizedWorkloadSecretMaterialization::from_validated_workload(
            ValidatedSecretMaterializationProjection {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                workload_id: WorkloadId::new(),
                workload_revision_id: WorkloadRevisionId::new(),
                node_id: NodeId::new(),
                secret_id: SecretId::new(),
                secret_version: 1,
            },
        )
        .expect("published authorization");
        assert!(fact.validate().is_ok());

        let mut value = serde_json::to_value(fact).expect("serialize authorization");
        value["secretVersion"] = serde_json::json!(0);
        let invalid: AuthorizedWorkloadSecretMaterialization =
            serde_json::from_value(value).expect("deserialize invalid fixture");
        assert!(invalid.validate().is_err());
    }
}
