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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    fn node(id: &str, kind: NodeKind) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            kind,
            position: Position { x: 0.0, y: 0.0 },
            data: NodeData {
                label: id.to_string(),
                config: json!({}),
                runtime: NodeRuntimePolicy::default(),
            },
        }
    }

    fn graph() -> (Vec<WorkflowNode>, Vec<WorkflowEdge>) {
        (
            vec![
                node("start", NodeKind::Start),
                node("output", NodeKind::Output),
            ],
            vec![WorkflowEdge {
                id: "edge".to_string(),
                source: "start".to_string(),
                target: "output".to_string(),
                source_handle: None,
            }],
        )
    }

    #[test]
    fn workflow_models_delegate_to_name_and_graph_validation() {
        let (nodes, edges) = graph();
        let draft = WorkflowDraft {
            name: " Demo ".to_string(),
            description: String::new(),
            nodes: nodes.clone(),
            edges: edges.clone(),
        };
        draft.validate().expect("valid draft");

        let update = WorkflowUpdate {
            version: 1,
            name: "Demo".to_string(),
            description: String::new(),
            nodes: nodes.clone(),
            edges: edges.clone(),
        };
        update.validate().expect("valid update");

        let now = Utc::now();
        let definition = WorkflowDefinition {
            id: "workflow".to_string(),
            name: "Demo".to_string(),
            description: String::new(),
            version: 1,
            nodes,
            edges,
            created_at: now,
            updated_at: now,
        };
        definition.validate().expect("valid definition");
    }

    #[test]
    fn workflow_name_must_have_one_to_120_trimmed_characters() {
        for invalid in [String::new(), "   ".to_string(), "x".repeat(121)] {
            let error = validate_name(&invalid).expect_err("name must fail");
            assert!(error.to_string().contains("between 1 and 120"));
        }

        validate_name(&"界".repeat(120)).expect("120 Unicode characters");
    }
}
