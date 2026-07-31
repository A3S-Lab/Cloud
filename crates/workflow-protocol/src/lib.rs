//! Stable wire contract shared by the Workflow control plane and Runtime nodes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const NODE_INVOCATION_SCHEMA: &str = "a3s.workflow.node-invocation.v1";
pub const NODE_RESULT_SCHEMA: &str = "a3s.workflow.node-result.v1";
pub const NODE_INVOCATION_MEDIA_TYPE: &str = "application/vnd.a3s.workflow.node-invocation.v1+json";
pub const NODE_RESULT_MEDIA_TYPE: &str = "application/vnd.a3s.workflow.node-result.v1+json";
pub const NODE_INVOCATION_PATH: &str = "/a3s/input/invocation.json";
pub const NODE_RESULT_PATH: &str = "/a3s/output/result.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Start,
    Template,
    Llm,
    Agent,
    Tool,
    Router,
    Memory,
    Http,
    Approval,
    Output,
}

impl NodeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Template => "template",
            Self::Llm => "llm",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::Router => "router",
            Self::Memory => "memory",
            Self::Http => "http",
            Self::Approval => "approval",
            Self::Output => "output",
        }
    }

    pub const fn requires_outbound_network(self) -> bool {
        matches!(
            self,
            Self::Llm | Self::Agent | Self::Tool | Self::Memory | Self::Http
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeData {
    pub label: String,
    #[serde(default = "empty_object")]
    pub config: Value,
    #[serde(default)]
    pub runtime: NodeRuntimePolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeRuntimePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pids: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<NodeIsolation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NodeNetworkMode>,
    #[serde(default)]
    pub secrets: Vec<NodeSecretReference>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeIsolation {
    Process,
    Container,
    Sandbox,
    Confidential,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeNetworkMode {
    None,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeSecretReference {
    pub name: String,
    pub reference: String,
    pub target: NodeSecretTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeSecretTarget {
    Environment { variable: String },
    File { path: String, mode: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: NodeKind,
    pub position: Position,
    pub data: NodeData,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_handle: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeExecutionPhase {
    Execute,
    Resume,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeServiceContext {
    pub gateway_base_url: String,
    pub default_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_base_url: Option<String>,
    #[serde(default)]
    pub http_allowed_hosts: Vec<String>,
    pub max_http_response_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeInvocation {
    pub schema: String,
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub workflow_id: String,
    pub workflow_version: u64,
    pub phase: NodeExecutionPhase,
    pub node: WorkflowNode,
    #[serde(default)]
    pub workflow_input: Value,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_payload: Option<Value>,
    pub services: NodeServiceContext,
}

impl NodeInvocation {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NODE_INVOCATION_SCHEMA {
            return Err(format!(
                "unsupported node invocation schema {:?}",
                self.schema
            ));
        }
        for (label, value) in [
            ("run_id", self.run_id.as_str()),
            ("step_id", self.step_id.as_str()),
            ("workflow_id", self.workflow_id.as_str()),
            ("node_id", self.node.id.as_str()),
        ] {
            if value.trim().is_empty() || value.len() > 512 {
                return Err(format!("{label} must be a bounded non-empty value"));
            }
        }
        if self.workflow_version == 0 {
            return Err("workflow_version must be positive".to_string());
        }
        if self.attempt == 0 {
            return Err("attempt must be positive".to_string());
        }
        if self.phase == NodeExecutionPhase::Resume && self.resume_payload.is_none() {
            return Err("resume phase requires resume_payload".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutionResult {
    pub schema: String,
    #[serde(default)]
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspension: Option<NodeSuspension>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl NodeExecutionResult {
    pub fn completed(output: Value) -> Self {
        Self {
            schema: NODE_RESULT_SCHEMA.to_string(),
            output,
            route: None,
            suspension: None,
            metadata: Map::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NODE_RESULT_SCHEMA {
            return Err(format!("unsupported node result schema {:?}", self.schema));
        }
        if self
            .route
            .as_ref()
            .is_some_and(|route| route.trim().is_empty())
        {
            return Err("route must be non-empty when present".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeSuspension {
    HumanApproval {
        message: String,
        #[serde(default)]
        details: Value,
    },
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_rejects_resume_without_payload() {
        let invocation = NodeInvocation {
            schema: NODE_INVOCATION_SCHEMA.to_string(),
            run_id: "run-1".to_string(),
            step_id: "node:start:resume".to_string(),
            attempt: 1,
            workflow_id: "workflow-1".to_string(),
            workflow_version: 1,
            phase: NodeExecutionPhase::Resume,
            node: WorkflowNode {
                id: "start".to_string(),
                kind: NodeKind::Start,
                position: Position { x: 0.0, y: 0.0 },
                data: NodeData {
                    label: "Start".to_string(),
                    config: Value::Null,
                    runtime: NodeRuntimePolicy::default(),
                },
            },
            workflow_input: Value::Null,
            dependencies: BTreeMap::new(),
            resume_payload: None,
            services: NodeServiceContext {
                gateway_base_url: "http://gateway.local/v1".to_string(),
                default_model: "test".to_string(),
                memory_base_url: None,
                http_allowed_hosts: Vec::new(),
                max_http_response_bytes: 1024,
            },
        };

        assert!(invocation.validate().is_err());
    }
}
