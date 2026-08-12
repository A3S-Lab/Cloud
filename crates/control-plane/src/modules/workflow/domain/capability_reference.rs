use super::validation::{validate_revision, validate_text};
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityOwner {
    Assets,
    Forms,
    Workflow,
    Inference,
    Use,
    Executions,
}

impl CapabilityOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assets => "assets",
            Self::Forms => "forms",
            Self::Workflow => "workflow",
            Self::Inference => "inference",
            Self::Use => "use",
            Self::Executions => "executions",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "assets" => Ok(Self::Assets),
            "forms" => Ok(Self::Forms),
            "workflow" => Ok(Self::Workflow),
            "inference" => Ok(Self::Inference),
            "use" => Ok(Self::Use),
            "executions" => Ok(Self::Executions),
            _ => Err(format!("unsupported Workflow capability owner {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityType {
    AgentRelease,
    McpServiceProfile,
    FormRelease,
    WorkflowRevision,
    ModelRevision,
    UsePackage,
    ExecutionTemplate,
    ConnectorRevision,
}

impl CapabilityType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentRelease => "agent_release",
            Self::McpServiceProfile => "mcp_service_profile",
            Self::FormRelease => "form_release",
            Self::WorkflowRevision => "workflow_revision",
            Self::ModelRevision => "model_revision",
            Self::UsePackage => "use_package",
            Self::ExecutionTemplate => "execution_template",
            Self::ConnectorRevision => "connector_revision",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "agent_release" => Ok(Self::AgentRelease),
            "mcp_service_profile" => Ok(Self::McpServiceProfile),
            "form_release" => Ok(Self::FormRelease),
            "workflow_revision" => Ok(Self::WorkflowRevision),
            "model_revision" => Ok(Self::ModelRevision),
            "use_package" => Ok(Self::UsePackage),
            "execution_template" => Ok(Self::ExecutionTemplate),
            "connector_revision" => Ok(Self::ConnectorRevision),
            _ => Err(format!("unsupported Workflow capability type {value:?}")),
        }
    }

    pub const fn owner(self) -> CapabilityOwner {
        match self {
            Self::AgentRelease | Self::McpServiceProfile => CapabilityOwner::Assets,
            Self::FormRelease => CapabilityOwner::Forms,
            Self::WorkflowRevision | Self::ConnectorRevision => CapabilityOwner::Workflow,
            Self::ModelRevision => CapabilityOwner::Inference,
            Self::UsePackage => CapabilityOwner::Use,
            Self::ExecutionTemplate => CapabilityOwner::Executions,
        }
    }
}

/// Exact, federated reference to a capability owned by another Cloud context.
///
/// Workflow stores this identity and digest but never copies the referenced
/// release, provider configuration, credential, or execution lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReference {
    pub owner: CapabilityOwner,
    #[serde(rename = "type")]
    pub capability_type: CapabilityType,
    pub resource_id: Uuid,
    pub revision: String,
    pub digest: Sha256Digest,
    pub capability: String,
}

impl CapabilityReference {
    pub fn validate(&self) -> Result<(), String> {
        if self.resource_id.is_nil() {
            return Err("Workflow capability resource ID must not be nil".into());
        }
        if self.owner != self.capability_type.owner() {
            return Err(format!(
                "Workflow capability type {} belongs to {}, not {}",
                self.capability_type.as_str(),
                self.capability_type.owner().as_str(),
                self.owner.as_str()
            ));
        }
        validate_revision("Workflow capability revision", &self.revision)?;
        if matches!(
            self.capability_type,
            CapabilityType::FormRelease | CapabilityType::ExecutionTemplate
        ) {
            let release_id = Uuid::parse_str(&self.revision).map_err(|_| {
                format!(
                    "Workflow {} capability revision must be an exact UUID",
                    self.capability_type.as_str()
                )
            })?;
            if release_id.is_nil() {
                return Err(format!(
                    "Workflow {} capability revision must be a non-nil UUID",
                    self.capability_type.as_str()
                ));
            }
        }
        validate_text("Workflow capability name", &self.capability, 1, 128)?;
        if !self.capability.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.' | b'/')
        }) {
            return Err("Workflow capability name must use portable lowercase syntax".into());
        }
        if self.capability_type == CapabilityType::ExecutionTemplate
            && self.capability != "execution.run"
        {
            return Err(
                "Workflow ExecutionTemplate capability must be exactly execution.run".into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest")
    }

    #[test]
    fn every_capability_type_has_one_authoritative_owner() {
        let cases = [
            (CapabilityType::AgentRelease, CapabilityOwner::Assets),
            (CapabilityType::McpServiceProfile, CapabilityOwner::Assets),
            (CapabilityType::FormRelease, CapabilityOwner::Forms),
            (CapabilityType::WorkflowRevision, CapabilityOwner::Workflow),
            (CapabilityType::ConnectorRevision, CapabilityOwner::Workflow),
            (CapabilityType::ModelRevision, CapabilityOwner::Inference),
            (CapabilityType::UsePackage, CapabilityOwner::Use),
            (
                CapabilityType::ExecutionTemplate,
                CapabilityOwner::Executions,
            ),
        ];

        for (capability_type, owner) in cases {
            assert_eq!(capability_type.owner(), owner);
            assert_eq!(
                CapabilityType::parse(capability_type.as_str()).expect("type"),
                capability_type
            );
            assert_eq!(
                CapabilityOwner::parse(owner.as_str()).expect("owner"),
                owner
            );
        }
    }

    #[test]
    fn references_fail_closed_on_owner_or_identity_drift() {
        let mut reference = CapabilityReference {
            owner: CapabilityOwner::Assets,
            capability_type: CapabilityType::AgentRelease,
            resource_id: Uuid::now_v7(),
            revision: "release-1".into(),
            digest: digest(),
            capability: "agent.execute".into(),
        };
        reference.validate().expect("valid reference");

        reference.owner = CapabilityOwner::Workflow;
        assert!(reference.validate().is_err());
        reference.owner = CapabilityOwner::Assets;
        reference.resource_id = Uuid::nil();
        assert!(reference.validate().is_err());
        reference.resource_id = Uuid::now_v7();
        reference.capability = "Agent Execute".into();
        assert!(reference.validate().is_err());
        reference.capability = "agent.execute".into();
        reference.revision = "unsafe/revision".into();
        assert!(reference.validate().is_err());

        reference.owner = CapabilityOwner::Forms;
        reference.capability_type = CapabilityType::FormRelease;
        reference.revision = "latest".into();
        assert!(reference.validate().is_err());
        reference.revision = Uuid::now_v7().to_string();
        reference.validate().expect("exact FormRelease reference");
    }
}
