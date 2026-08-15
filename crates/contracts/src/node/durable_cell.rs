use super::{validate_lower_sha256, validate_single_line, validate_uuid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const NODE_DURABLE_CELL_OPERATOR_OBSERVE_SCHEMA_V1: &str =
    "a3s.cloud.durable-cell-operator-observe.v1";

/// Exact ordinary Runtime Service generation that exposes a Durable Cell
/// provider's node-local operator endpoint.
///
/// This is only a correlation fence around identities owned by Durable Cells,
/// Workloads, and Runtime. It is not provider configuration, a deployment
/// record, or a second Runtime lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDurableCellOperatorBindingV1 {
    pub schema: String,
    pub application_id: Uuid,
    pub application_revision_id: Uuid,
    pub application_revision_number: u64,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub runtime_spec_digest: String,
    pub service_profile_digest: String,
    pub service_template_digest: String,
    pub provider_artifact_digest: String,
    pub internal_service_port_name: String,
}

impl NodeDurableCellOperatorBindingV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.durable-cell-operator-binding.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Durable Cell operator binding schema {:?}",
                self.schema
            ));
        }
        validate_uuid("application_id", self.application_id)?;
        validate_uuid("application_revision_id", self.application_revision_id)?;
        validate_uuid("workload_id", self.workload_id)?;
        validate_uuid("workload_revision_id", self.workload_revision_id)?;
        if self.application_revision_number == 0 || self.runtime_generation == 0 {
            return Err("Durable Cell revision and Runtime generation must be positive".into());
        }
        validate_single_line("Runtime unit ID", &self.runtime_unit_id, 512)?;
        validate_lower_sha256("Runtime spec digest", &self.runtime_spec_digest)?;
        validate_lower_sha256("Service profile digest", &self.service_profile_digest)?;
        validate_lower_sha256("Service template digest", &self.service_template_digest)?;
        validate_lower_sha256("provider artifact digest", &self.provider_artifact_digest)?;
        validate_single_line(
            "Durable Cell internal service port name",
            &self.internal_service_port_name,
            128,
        )
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode Durable Cell operator binding: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

/// Sanitized, bounded view of a provider's node-local operator state.
///
/// Provider-native phase names, Cell names, resident sets, published sets,
/// and raw response bytes are deliberately absent. Those values never become
/// Fleet receipts or Cloud state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDurableCellOperatorObservationV1 {
    pub schema: String,
    pub binding_digest: String,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub runtime_spec_digest: String,
    pub occupied: u32,
    pub evicting: u32,
    pub restoring: u32,
    pub activating: u32,
    pub activation_waiting: u32,
    pub capacity_waiting: u32,
    pub observed_at_ms: u64,
}

impl NodeDurableCellOperatorObservationV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.durable-cell-operator-observation.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Durable Cell operator observation schema {:?}",
                self.schema
            ));
        }
        validate_lower_sha256("Durable Cell operator binding digest", &self.binding_digest)?;
        validate_single_line("Runtime unit ID", &self.runtime_unit_id, 512)?;
        if self.runtime_generation == 0 || self.observed_at_ms == 0 {
            return Err(
                "Durable Cell operator Runtime generation and observation time must be positive"
                    .into(),
            );
        }
        validate_lower_sha256("Runtime spec digest", &self.runtime_spec_digest)
    }

    pub fn validate_for(&self, binding: &NodeDurableCellOperatorBindingV1) -> Result<(), String> {
        binding.validate()?;
        self.validate()?;
        if self.binding_digest != binding.digest()?
            || self.runtime_unit_id != binding.runtime_unit_id
            || self.runtime_generation != binding.runtime_generation
            || self.runtime_spec_digest != binding.runtime_spec_digest
        {
            return Err(
                "Durable Cell operator observation changed its exact Runtime binding".into(),
            );
        }
        Ok(())
    }
}
