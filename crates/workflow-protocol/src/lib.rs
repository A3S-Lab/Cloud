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
    use serde_json::json;

    use super::*;

    fn invocation() -> NodeInvocation {
        NodeInvocation {
            schema: NODE_INVOCATION_SCHEMA.to_string(),
            run_id: "run-1".to_string(),
            step_id: "node:start:execute".to_string(),
            attempt: 1,
            workflow_id: "workflow-1".to_string(),
            workflow_version: 1,
            phase: NodeExecutionPhase::Execute,
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
        }
    }

    fn assert_invalid(invocation: &NodeInvocation, message: &str) {
        let error = invocation.validate().expect_err("invocation must fail");
        assert!(
            error.contains(message),
            "expected {error:?} to contain {message:?}"
        );
    }

    #[test]
    fn node_kind_names_and_network_requirements_are_stable() {
        let cases = [
            (NodeKind::Start, "start", false),
            (NodeKind::Template, "template", false),
            (NodeKind::Llm, "llm", true),
            (NodeKind::Agent, "agent", true),
            (NodeKind::Tool, "tool", true),
            (NodeKind::Router, "router", false),
            (NodeKind::Memory, "memory", true),
            (NodeKind::Http, "http", true),
            (NodeKind::Approval, "approval", false),
            (NodeKind::Output, "output", false),
        ];

        for (kind, name, outbound) in cases {
            assert_eq!(kind.as_str(), name);
            assert_eq!(kind.requires_outbound_network(), outbound);
            assert_eq!(
                serde_json::to_value(kind).expect("serialize kind"),
                json!(name)
            );
        }
    }

    #[test]
    fn runtime_policy_matches_the_documented_wire_contract() {
        let source = json!({
            "provider": "production",
            "pool": "gpu-a100",
            "cpuMillis": 2000,
            "memoryBytes": 4294967296_u64,
            "pids": 256,
            "timeoutMs": 120000,
            "isolation": "container",
            "network": "outbound",
            "secrets": [{
                "name": "openai-api-key",
                "reference": "env://OPENAI_API_KEY",
                "target": {
                    "kind": "environment",
                    "variable": "OPENAI_API_KEY"
                }
            }]
        });
        let policy: NodeRuntimePolicy =
            serde_json::from_value(source.clone()).expect("deserialize runtime policy");

        assert_eq!(policy.provider.as_deref(), Some("production"));
        assert_eq!(policy.pool.as_deref(), Some("gpu-a100"));
        assert_eq!(policy.isolation, Some(NodeIsolation::Container));
        assert_eq!(policy.network, Some(NodeNetworkMode::Outbound));
        assert_eq!(
            serde_json::to_value(policy).expect("serialize runtime policy"),
            source
        );
    }

    #[test]
    fn node_data_defaults_to_object_config_and_empty_runtime_policy() {
        let data: NodeData = serde_json::from_value(json!({ "label": "Start" }))
            .expect("deserialize node data defaults");

        assert_eq!(data.config, json!({}));
        assert_eq!(data.runtime, NodeRuntimePolicy::default());
    }

    #[test]
    fn invocation_accepts_execute_and_resume_with_payload() {
        let execute = invocation();
        execute.validate().expect("execute invocation");

        let mut resume = invocation();
        resume.phase = NodeExecutionPhase::Resume;
        resume.resume_payload = Some(json!({ "approved": true }));
        resume.validate().expect("resume invocation");
    }

    #[test]
    fn invocation_rejects_unknown_schema_and_unbounded_identity() {
        let mut value = invocation();
        value.schema = "unknown".to_string();
        assert_invalid(&value, "unsupported node invocation schema");

        for field in ["run_id", "step_id", "workflow_id", "node_id"] {
            let mut value = invocation();
            match field {
                "run_id" => value.run_id = "   ".to_string(),
                "step_id" => value.step_id = "x".repeat(513),
                "workflow_id" => value.workflow_id.clear(),
                "node_id" => value.node.id.clear(),
                _ => unreachable!(),
            }
            assert_invalid(&value, field);
        }
    }

    #[test]
    fn invocation_rejects_zero_version_attempt_and_missing_resume_payload() {
        let mut value = invocation();
        value.workflow_version = 0;
        assert_invalid(&value, "workflow_version must be positive");

        let mut value = invocation();
        value.attempt = 0;
        assert_invalid(&value, "attempt must be positive");

        let mut value = invocation();
        value.phase = NodeExecutionPhase::Resume;
        assert_invalid(&value, "resume phase requires resume_payload");
    }

    #[test]
    fn completed_result_is_valid_and_uses_protocol_defaults() {
        let result = NodeExecutionResult::completed(json!({ "ok": true }));

        result.validate().expect("completed result");
        assert_eq!(result.schema, NODE_RESULT_SCHEMA);
        assert_eq!(result.output, json!({ "ok": true }));
        assert!(result.route.is_none());
        assert!(result.suspension.is_none());
        assert!(result.metadata.is_empty());
    }

    #[test]
    fn result_rejects_unknown_schema_and_blank_route() {
        let mut result = NodeExecutionResult::completed(Value::Null);
        result.schema = "unknown".to_string();
        assert!(result
            .validate()
            .expect_err("schema must fail")
            .contains("unsupported node result schema"));

        let mut result = NodeExecutionResult::completed(Value::Null);
        result.route = Some("  ".to_string());
        assert!(result
            .validate()
            .expect_err("blank route must fail")
            .contains("route must be non-empty"));
    }
}
