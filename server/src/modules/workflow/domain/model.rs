pub use a3s_workflow_protocol::{
    NodeData, NodeIsolation, NodeKind, NodeNetworkMode, NodeRuntimePolicy, NodeSecretReference,
    NodeSecretTarget, Position, WorkflowEdge, WorkflowNode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{topological_order, WorkflowResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: u64,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDraft {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowUpdate {
    pub version: u64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

impl WorkflowDefinition {
    pub fn validate(&self) -> WorkflowResult<()> {
        validate_name(&self.name)?;
        topological_order(&self.nodes, &self.edges).map(|_| ())
    }
}

impl WorkflowDraft {
    pub fn validate(&self) -> WorkflowResult<()> {
        validate_name(&self.name)?;
        topological_order(&self.nodes, &self.edges).map(|_| ())
    }
}

impl WorkflowUpdate {
    pub fn validate(&self) -> WorkflowResult<()> {
        validate_name(&self.name)?;
        topological_order(&self.nodes, &self.edges).map(|_| ())
    }
}

fn validate_name(name: &str) -> WorkflowResult<()> {
    let length = name.trim().chars().count();
    if !(1..=120).contains(&length) {
        return Err(super::WorkflowError::Validation(
            "name must contain between 1 and 120 characters".to_string(),
        ));
    }
    Ok(())
}
