use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a3s_flow::{
    FlowError, FlowEvent, FlowRuntime, HookCallbackRoute, HookMetadata, RuntimeCommand,
    StepInvocation, WorkflowInvocation,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeApplyRequest,
    RuntimeMount, RuntimeMountSource, RuntimeNetworkSpec, RuntimeOutputSpec, RuntimeProcessSpec,
    RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState, SecretReference, SecretTarget,
};
use a3s_runtime::RuntimeClient;
use a3s_workflow_protocol::{
    NodeExecutionPhase, NodeExecutionResult, NodeInvocation, NodeIsolation, NodeKind,
    NodeNetworkMode, NodeSecretTarget, NodeServiceContext, NodeSuspension, WorkflowEdge,
    WorkflowNode, NODE_INVOCATION_MEDIA_TYPE, NODE_INVOCATION_PATH, NODE_INVOCATION_SCHEMA,
    NODE_RESULT_MEDIA_TYPE, NODE_RESULT_PATH,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::node_execution_store::{digest, SharedNodeExecutionStore};
use super::remote_runtime::RemoteRuntimeClient;
use crate::config::{GatewayConfig, MemoryConfig, RuntimeConfig, RuntimeProviderConfig};
use crate::modules::workflow::domain::{topological_order, WorkflowDefinition, WorkflowError};

#[derive(Debug, Clone)]
pub struct GraphRuntimeConfig {
    pub runtime: RuntimeConfig,
    pub providers: BTreeMap<String, RuntimeProviderConfig>,
    pub gateway: GatewayConfig,
    pub memory: MemoryConfig,
    pub http_allowed_hosts: Vec<String>,
    pub max_http_response_bytes: usize,
}

#[derive(Clone)]
pub struct GraphRuntime {
    config: Arc<GraphRuntimeConfig>,
    providers: Arc<BTreeMap<String, Arc<dyn RuntimeClient>>>,
    executions: SharedNodeExecutionStore,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphRunInput {
    definition: WorkflowDefinition,
    input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphStepInput {
    workflow_id: String,
    workflow_version: u64,
    phase: NodeExecutionPhase,
    node: WorkflowNode,
    workflow_input: Value,
    dependencies: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resume_payload: Option<Value>,
}

#[derive(Debug, Clone)]
enum ResolvedNode {
    Active(NodeExecutionResult),
    Inactive,
}

impl GraphRuntime {
    pub fn new(
        config: GraphRuntimeConfig,
        executions: SharedNodeExecutionStore,
    ) -> Result<Self, FlowError> {
        let providers = config
            .providers
            .iter()
            .map(|(name, provider)| {
                let client = RemoteRuntimeClient::new(&provider.endpoint, &provider.api_token)
                    .map_err(|error| FlowError::Runtime(error.to_string()))?;
                Ok((name.clone(), Arc::new(client) as Arc<dyn RuntimeClient>))
            })
            .collect::<Result<BTreeMap<_, _>, FlowError>>()?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| FlowError::Runtime(format!("failed to build HTTP client: {error}")))?;
        Ok(Self {
            config: Arc::new(config),
            providers: Arc::new(providers),
            executions,
            client,
        })
    }

    fn services(&self) -> NodeServiceContext {
        NodeServiceContext {
            gateway_base_url: self.config.gateway.base_url.clone(),
            default_model: self.config.gateway.default_model.clone(),
            memory_base_url: Some(self.config.memory.base_url.clone()),
            http_allowed_hosts: self.config.http_allowed_hosts.clone(),
            max_http_response_bytes: self.config.max_http_response_bytes,
        }
    }

    fn provider_selector(&self, node: &WorkflowNode) -> String {
        let provider = node
            .data
            .runtime
            .provider
            .as_deref()
            .unwrap_or(&self.config.runtime.default_provider);
        match node.data.runtime.pool.as_deref() {
            Some(pool) => format!("{provider}-{pool}"),
            None => provider.to_string(),
        }
    }

    fn incoming(definition: &WorkflowDefinition) -> BTreeMap<&str, Vec<&WorkflowEdge>> {
        let mut incoming: BTreeMap<&str, Vec<&WorkflowEdge>> = definition
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), Vec::new()))
            .collect();
        for edge in &definition.edges {
            if let Some(edges) = incoming.get_mut(edge.target.as_str()) {
                edges.push(edge);
                edges.sort_by(|left, right| left.id.cmp(&right.id));
            }
        }
        incoming
    }

    fn step_id(node_id: &str, phase: NodeExecutionPhase) -> String {
        let phase = match phase {
            NodeExecutionPhase::Execute => "execute",
            NodeExecutionPhase::Resume => "resume",
        };
        format!("node:{node_id}:{phase}")
    }

    fn completed_result(
        context: &a3s_flow::WorkflowContext<'_>,
        node_id: &str,
        phase: NodeExecutionPhase,
    ) -> Result<Option<NodeExecutionResult>, FlowError> {
        context
            .step_output(&Self::step_id(node_id, phase))
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }

    fn dependency_state(
        node: &WorkflowNode,
        incoming: &BTreeMap<&str, Vec<&WorkflowEdge>>,
        resolved: &BTreeMap<&str, ResolvedNode>,
        node_by_id: &BTreeMap<&str, &WorkflowNode>,
    ) -> Result<Option<Option<BTreeMap<String, Value>>>, FlowError> {
        let edges = incoming.get(node.id.as_str()).ok_or_else(|| {
            FlowError::InvalidWorkflow(format!("node {} is missing incoming edge state", node.id))
        })?;
        if edges.is_empty() {
            return Ok(Some(Some(BTreeMap::new())));
        }

        let mut dependencies = BTreeMap::new();
        let mut has_active_edge = false;
        for edge in edges {
            let Some(source) = resolved.get(edge.source.as_str()) else {
                return Ok(None);
            };
            let ResolvedNode::Active(result) = source else {
                continue;
            };
            let source_node = node_by_id.get(edge.source.as_str()).ok_or_else(|| {
                FlowError::InvalidWorkflow(format!("edge {} has no source node", edge.id))
            })?;
            let active = if source_node.kind == NodeKind::Router {
                result.route.as_deref() == edge.source_handle.as_deref()
            } else {
                true
            };
            if active {
                has_active_edge = true;
                dependencies.insert(edge.source.clone(), result.output.clone());
            }
        }
        if has_active_edge {
            Ok(Some(Some(dependencies)))
        } else {
            Ok(Some(None))
        }
    }

    fn graph_step(
        &self,
        definition: &WorkflowDefinition,
        input: &Value,
        node: &WorkflowNode,
        phase: NodeExecutionPhase,
        dependencies: BTreeMap<String, Value>,
        resume_payload: Option<Value>,
    ) -> Result<a3s_flow::StepCommand, FlowError> {
        let step_id = Self::step_id(&node.id, phase);
        let payload = serde_json::to_value(GraphStepInput {
            workflow_id: definition.id.clone(),
            workflow_version: definition.version,
            phase,
            node: node.clone(),
            workflow_input: input.clone(),
            dependencies,
            resume_payload,
        })?;
        Ok(a3s_flow::StepCommand::new(
            step_id,
            format!("{}:{}", node.kind.as_str(), phase_name(phase)),
            payload,
        ))
    }

    async fn dispatch(&self, invocation: NodeInvocation) -> Result<NodeExecutionResult, FlowError> {
        invocation
            .validate()
            .map_err(|error| FlowError::Runtime(format!("invalid node invocation: {error}")))?;
        let selector = self.provider_selector(&invocation.node);
        let provider = self.providers.get(&selector).ok_or_else(|| {
            FlowError::Runtime(format!(
                "node {} selected unconfigured Runtime provider {selector:?}",
                invocation.node.id
            ))
        })?;
        let prepared = self
            .executions
            .prepare(
                &invocation,
                &selector,
                invocation.node.data.runtime.pool.as_deref(),
            )
            .await
            .map_err(workflow_execution_error)?;
        let spec = self.runtime_spec(&invocation, &prepared)?;
        let spec_digest = spec
            .digest()
            .map_err(|error| FlowError::Runtime(format!("invalid Runtime unit: {error}")))?;
        self.executions
            .mark_dispatched(
                &prepared.execution_id,
                &spec.unit_id,
                spec.generation,
                &spec_digest,
            )
            .await
            .map_err(workflow_execution_error)?;
        let request = RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.to_string(),
            request_id: format!("apply/{}", prepared.execution_id),
            deadline_at_ms: Some(
                now_ms().saturating_add(
                    invocation
                        .node
                        .data
                        .runtime
                        .timeout_ms
                        .unwrap_or(self.config.runtime.default_timeout_ms)
                        .saturating_add(30_000),
                ),
            ),
            spec: spec.clone(),
        };
        let observation = provider
            .apply(&request)
            .await
            .map_err(|error| FlowError::Runtime(error.to_string()))?;
        observation
            .validate_against(&spec)
            .map_err(|error| FlowError::Runtime(format!("invalid Runtime evidence: {error}")))?;
        self.executions
            .complete(&prepared.execution_id, &observation)
            .await
            .map_err(workflow_execution_error)?;
        if observation.state != RuntimeUnitState::Succeeded {
            let message = observation
                .failure
                .as_ref()
                .map(|failure| format!("{}: {}", failure.code, failure.message))
                .unwrap_or_else(|| format!("Runtime unit ended in {:?}", observation.state));
            return Err(FlowError::Runtime(message));
        }
        let output = observation
            .outputs
            .iter()
            .find(|output| output.name == "result")
            .ok_or_else(|| {
                FlowError::Runtime("Runtime omitted node result artifact".to_string())
            })?;
        let bytes = self
            .fetch_output(
                &output.artifact.uri,
                output.size_bytes,
                &output.artifact.digest,
            )
            .await?;
        let result: NodeExecutionResult = serde_json::from_slice(&bytes).map_err(|error| {
            FlowError::Runtime(format!(
                "Runtime returned invalid node result JSON: {error}"
            ))
        })?;
        result
            .validate()
            .map_err(|error| FlowError::Runtime(format!("invalid node result: {error}")))?;
        Ok(result)
    }

    fn runtime_spec(
        &self,
        invocation: &NodeInvocation,
        prepared: &super::node_execution_store::PreparedNodeExecution,
    ) -> Result<RuntimeUnitSpec, FlowError> {
        let timeout = invocation
            .node
            .data
            .runtime
            .timeout_ms
            .unwrap_or(self.config.runtime.default_timeout_ms);
        let generation = u64::from(invocation.attempt);
        let phase = phase_name(invocation.phase);
        let mut environment = BTreeMap::from([
            (
                "A3S_WORKFLOW_INVOCATION_PATH".to_string(),
                NODE_INVOCATION_PATH.to_string(),
            ),
            (
                "A3S_WORKFLOW_RESULT_PATH".to_string(),
                NODE_RESULT_PATH.to_string(),
            ),
            (
                "A3S_WORKFLOW_NODE_KIND".to_string(),
                invocation.node.kind.as_str().to_string(),
            ),
        ]);
        if let Some(pool) = &invocation.node.data.runtime.pool {
            environment.insert("A3S_RUNTIME_POOL".to_string(), pool.clone());
        }
        let invocation_uri = format!(
            "{}/internal/v1/node-executions/{}/invocation?token={}",
            self.config
                .runtime
                .invocation_base_url
                .trim_end_matches('/'),
            prepared.execution_id,
            prepared.token
        );
        let spec = RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.to_string(),
            unit_id: format!(
                "workflow/{}/node/{}/{}",
                invocation.run_id, invocation.node.id, phase
            ),
            generation,
            class: RuntimeUnitClass::Task,
            artifact: ArtifactRef {
                uri: self.config.runtime.node_artifact_uri.clone(),
                digest: self.config.runtime.node_artifact_digest.clone(),
                media_type: self.config.runtime.node_artifact_media_type.clone(),
            },
            process: RuntimeProcessSpec {
                command: self.config.runtime.node_command.clone(),
                args: Vec::new(),
                working_directory: None,
                environment,
            },
            mounts: vec![RuntimeMount {
                name: "invocation".to_string(),
                source: RuntimeMountSource::Artifact {
                    artifact: ArtifactRef {
                        uri: invocation_uri,
                        digest: prepared.invocation_digest.clone(),
                        media_type: NODE_INVOCATION_MEDIA_TYPE.to_string(),
                    },
                },
                target: NODE_INVOCATION_PATH.to_string(),
                read_only: true,
            }],
            secrets: self.runtime_secrets(invocation)?,
            network: RuntimeNetworkSpec {
                mode: network_mode(invocation),
                ports: Vec::new(),
            },
            resources: ResourceLimits {
                cpu_millis: invocation
                    .node
                    .data
                    .runtime
                    .cpu_millis
                    .unwrap_or(self.config.runtime.default_cpu_millis),
                memory_bytes: invocation
                    .node
                    .data
                    .runtime
                    .memory_bytes
                    .unwrap_or(self.config.runtime.default_memory_bytes),
                pids: invocation
                    .node
                    .data
                    .runtime
                    .pids
                    .unwrap_or(self.config.runtime.default_pids),
                ephemeral_storage_bytes: Some(self.config.runtime.output_max_bytes * 4),
                execution_timeout_ms: Some(timeout),
            },
            isolation: isolation_level(invocation),
            health: None,
            restart: RestartPolicy::Never,
            outputs: vec![RuntimeOutputSpec {
                name: "result".to_string(),
                path: NODE_RESULT_PATH.to_string(),
                media_type: NODE_RESULT_MEDIA_TYPE.to_string(),
                max_bytes: self.config.runtime.output_max_bytes,
            }],
            semantics_profile_digest: Some(semantics_digest(
                &self.config.runtime.node_artifact_digest,
            )),
        };
        spec.validate()
            .map_err(|error| FlowError::Runtime(format!("invalid Runtime unit: {error}")))?;
        Ok(spec)
    }

    fn runtime_secrets(
        &self,
        invocation: &NodeInvocation,
    ) -> Result<Vec<SecretReference>, FlowError> {
        let mut secrets = invocation
            .node
            .data
            .runtime
            .secrets
            .iter()
            .map(|secret| SecretReference {
                name: secret.name.clone(),
                reference: secret.reference.clone(),
                target: match &secret.target {
                    NodeSecretTarget::Environment { variable } => SecretTarget::Environment {
                        variable: variable.clone(),
                    },
                    NodeSecretTarget::File { path, mode } => SecretTarget::File {
                        path: path.clone(),
                        mode: *mode,
                    },
                },
            })
            .collect::<Vec<_>>();
        if matches!(invocation.node.kind, NodeKind::Llm | NodeKind::Agent)
            && !self.config.gateway.api_key_reference.is_empty()
        {
            secrets.push(SecretReference {
                name: "a3s-gateway-api-key".to_string(),
                reference: self.config.gateway.api_key_reference.clone(),
                target: SecretTarget::Environment {
                    variable: "A3S_GATEWAY_API_KEY".to_string(),
                },
            });
        }
        if invocation.node.kind == NodeKind::Memory
            && !self.config.memory.api_key_reference.is_empty()
        {
            secrets.push(SecretReference {
                name: "a3s-memory-api-key".to_string(),
                reference: self.config.memory.api_key_reference.clone(),
                target: SecretTarget::Environment {
                    variable: "A3S_MEMORY_API_KEY".to_string(),
                },
            });
        }
        let unique_names = secrets
            .iter()
            .map(|secret| secret.name.as_str())
            .collect::<BTreeSet<_>>();
        if unique_names.len() != secrets.len() {
            return Err(FlowError::Runtime(format!(
                "node {} declares duplicate Runtime secret names",
                invocation.node.id
            )));
        }
        Ok(secrets)
    }

    async fn fetch_output(
        &self,
        uri: &str,
        reported_size: u64,
        expected_digest: &str,
    ) -> Result<Vec<u8>, FlowError> {
        if reported_size > self.config.runtime.output_max_bytes {
            return Err(FlowError::Runtime(
                "Runtime result exceeds configured output limit".to_string(),
            ));
        }
        let url = url::Url::parse(uri)
            .map_err(|error| FlowError::Runtime(format!("invalid output artifact URI: {error}")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(FlowError::Runtime(
                "Runtime result artifact must use http:// or https://".to_string(),
            ));
        }
        let response = self.client.get(url).send().await.map_err(|error| {
            FlowError::Runtime(format!("result artifact fetch failed: {error}"))
        })?;
        if !response.status().is_success() {
            return Err(FlowError::Runtime(format!(
                "result artifact fetch returned {}",
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| FlowError::Runtime(format!("result artifact read failed: {error}")))?;
        if bytes.len() as u64 > self.config.runtime.output_max_bytes {
            return Err(FlowError::Runtime(
                "Runtime result exceeds configured output limit".to_string(),
            ));
        }
        if digest(&bytes) != expected_digest {
            return Err(FlowError::Runtime(
                "Runtime result artifact digest mismatch".to_string(),
            ));
        }
        Ok(bytes.to_vec())
    }
}

#[async_trait]
impl FlowRuntime for GraphRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let run: GraphRunInput = invocation.input_as()?;
        run.definition
            .validate()
            .map_err(|error| FlowError::InvalidWorkflow(error.to_string()))?;
        let order = topological_order(&run.definition.nodes, &run.definition.edges)
            .map_err(|error| FlowError::InvalidWorkflow(error.to_string()))?;
        let incoming = Self::incoming(&run.definition);
        let node_by_id = run
            .definition
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect::<BTreeMap<_, _>>();
        let context = invocation.context();
        let mut resolved: BTreeMap<&str, ResolvedNode> = BTreeMap::new();
        let mut ready = Vec::new();

        for node_id in order {
            let node = node_by_id.get(node_id.as_str()).ok_or_else(|| {
                FlowError::InvalidWorkflow(format!("missing node {node_id} during execution"))
            })?;
            let Some(dependencies) =
                Self::dependency_state(node, &incoming, &resolved, &node_by_id)?
            else {
                continue;
            };
            let Some(dependencies) = dependencies else {
                resolved.insert(node.id.as_str(), ResolvedNode::Inactive);
                continue;
            };

            let execute_result =
                Self::completed_result(&context, &node.id, NodeExecutionPhase::Execute)?;
            if node.kind == NodeKind::Approval {
                if context.hook_disposed(&node.id) {
                    return Ok(context.fail(format!("approval node {} was disposed", node.id)));
                }
                if let Some(resume_result) =
                    Self::completed_result(&context, &node.id, NodeExecutionPhase::Resume)?
                {
                    ensure_no_suspension(node, &resume_result)?;
                    resolved.insert(node.id.as_str(), ResolvedNode::Active(resume_result));
                    continue;
                }
                if let Some(payload) = context.hook_payload(&node.id) {
                    ready.push(self.graph_step(
                        &run.definition,
                        &run.input,
                        node,
                        NodeExecutionPhase::Resume,
                        dependencies,
                        Some(payload.clone()),
                    )?);
                    continue;
                }
                if let Some(result) = execute_result {
                    let suspension = result.suspension.ok_or_else(|| {
                        FlowError::Runtime(format!(
                            "approval node {} completed without requesting approval",
                            node.id
                        ))
                    })?;
                    let NodeSuspension::HumanApproval { message, details } = suspension;
                    let token = Uuid::new_v5(
                        &Uuid::NAMESPACE_URL,
                        format!("a3s-workflow://{}/{}", invocation.run_id, node.id).as_bytes(),
                    )
                    .to_string();
                    let metadata = HookMetadata::human_approval(message)
                        .with_callback_route(HookCallbackRoute::post(format!(
                            "/api/v1/runs/{}/approvals/{}",
                            invocation.run_id, node.id
                        )))
                        .with_label("workflow_id", run.definition.id.clone())
                        .with_label("node_id", node.id.clone())
                        .with_data("details", details);
                    return context.create_hook_with_metadata(&node.id, token, metadata);
                }
            } else if let Some(result) = execute_result {
                ensure_no_suspension(node, &result)?;
                if node.kind == NodeKind::Router {
                    validate_route(node, &result, &run.definition.edges)?;
                }
                if node.kind == NodeKind::Output {
                    return Ok(context.complete(result.output));
                }
                resolved.insert(node.id.as_str(), ResolvedNode::Active(result));
                continue;
            }

            ready.push(self.graph_step(
                &run.definition,
                &run.input,
                node,
                NodeExecutionPhase::Execute,
                dependencies,
                None,
            )?);
        }

        if ready.is_empty() {
            Ok(context.fail("workflow graph stalled before its Runtime output node completed"))
        } else {
            Ok(context.schedule_steps(ready))
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<Value> {
        let input: GraphStepInput = invocation.input_as()?;
        let attempt = latest_attempt(&invocation)?;
        let node_invocation = NodeInvocation {
            schema: NODE_INVOCATION_SCHEMA.to_string(),
            run_id: invocation.run_id,
            step_id: invocation.step_id,
            attempt,
            workflow_id: input.workflow_id,
            workflow_version: input.workflow_version,
            phase: input.phase,
            node: input.node,
            workflow_input: input.workflow_input,
            dependencies: input.dependencies,
            resume_payload: input.resume_payload,
            services: self.services(),
        };
        let result = self.dispatch(node_invocation).await?;
        serde_json::to_value(result).map_err(FlowError::from)
    }
}

fn latest_attempt(invocation: &StepInvocation) -> Result<u32, FlowError> {
    invocation
        .history
        .iter()
        .rev()
        .find_map(|event| match &event.event {
            FlowEvent::StepStarted { step_id, attempt } if step_id == &invocation.step_id => {
                Some(*attempt)
            }
            _ => None,
        })
        .ok_or_else(|| {
            FlowError::Runtime(format!(
                "step {} has no durable attempt identity",
                invocation.step_id
            ))
        })
}

fn phase_name(phase: NodeExecutionPhase) -> &'static str {
    match phase {
        NodeExecutionPhase::Execute => "execute",
        NodeExecutionPhase::Resume => "resume",
    }
}

fn network_mode(invocation: &NodeInvocation) -> NetworkMode {
    match invocation.node.data.runtime.network {
        Some(NodeNetworkMode::None) => NetworkMode::None,
        Some(NodeNetworkMode::Outbound) => NetworkMode::Outbound,
        None if invocation.node.kind.requires_outbound_network() => NetworkMode::Outbound,
        None => NetworkMode::None,
    }
}

fn isolation_level(invocation: &NodeInvocation) -> IsolationLevel {
    match invocation.node.data.runtime.isolation {
        Some(NodeIsolation::Process) => IsolationLevel::Process,
        Some(NodeIsolation::Container) => IsolationLevel::Container,
        None => IsolationLevel::Process,
        Some(NodeIsolation::Sandbox) => IsolationLevel::Sandbox,
        Some(NodeIsolation::Confidential) => IsolationLevel::Confidential,
    }
}

fn semantics_digest(artifact_digest: &str) -> String {
    let source = format!(
        "{}\n{}\n{}",
        NODE_INVOCATION_SCHEMA,
        a3s_workflow_protocol::NODE_RESULT_SCHEMA,
        artifact_digest
    );
    format!("sha256:{:x}", Sha256::digest(source.as_bytes()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn ensure_no_suspension(
    node: &WorkflowNode,
    result: &NodeExecutionResult,
) -> Result<(), FlowError> {
    if result.suspension.is_some() {
        Err(FlowError::Runtime(format!(
            "non-pending node {} returned an unexpected suspension",
            node.id
        )))
    } else {
        Ok(())
    }
}

fn validate_route(
    node: &WorkflowNode,
    result: &NodeExecutionResult,
    edges: &[WorkflowEdge],
) -> Result<(), FlowError> {
    let route = result
        .route
        .as_deref()
        .ok_or_else(|| FlowError::Runtime(format!("router node {} returned no route", node.id)))?;
    let matches = edges
        .iter()
        .any(|edge| edge.source == node.id && edge.source_handle.as_deref() == Some(route));
    if matches {
        Ok(())
    } else {
        Err(FlowError::Runtime(format!(
            "router node {} selected unknown route {route:?}",
            node.id
        )))
    }
}

fn workflow_execution_error(error: WorkflowError) -> FlowError {
    FlowError::Runtime(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantics_profile_binds_protocol_and_runner_artifact() {
        assert_ne!(
            semantics_digest(&format!("sha256:{}", "a".repeat(64))),
            semantics_digest(&format!("sha256:{}", "b".repeat(64)))
        );
    }
}
