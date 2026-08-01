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
        runtime_selector(&self.config.runtime.default_provider, node)
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

fn runtime_selector(default_provider: &str, node: &WorkflowNode) -> String {
    let provider = node
        .data
        .runtime
        .provider
        .as_deref()
        .unwrap_or(default_provider);
    match node.data.runtime.pool.as_deref() {
        Some(pool) => format!("{provider}-{pool}"),
        None => provider.to_string(),
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
    use a3s_flow::{FlowEventEnvelope, RuntimeCommand, WorkflowSpec};
    use a3s_runtime::contract::{
        RuntimeActionRequest, RuntimeCapabilities, RuntimeExecRequest, RuntimeExecResult,
        RuntimeFailure, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation,
        RuntimeOutputArtifact, RuntimeRemoval,
    };
    use a3s_runtime::{RuntimeError, RuntimeResult};
    use a3s_workflow_protocol::{
        NodeData, NodeRuntimePolicy, NodeSecretReference, NodeSecretTarget, Position,
        NODE_RESULT_SCHEMA,
    };
    use chrono::Utc;
    use serde_json::{json, Map};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn invocation(kind: NodeKind) -> NodeInvocation {
        NodeInvocation {
            schema: NODE_INVOCATION_SCHEMA.to_string(),
            run_id: "run".to_string(),
            step_id: format!("node:{}:execute", kind.as_str()),
            attempt: 1,
            workflow_id: "workflow".to_string(),
            workflow_version: 1,
            phase: NodeExecutionPhase::Execute,
            node: WorkflowNode {
                id: kind.as_str().to_string(),
                kind,
                position: Position { x: 0.0, y: 0.0 },
                data: NodeData {
                    label: kind.as_str().to_string(),
                    config: json!({}),
                    runtime: NodeRuntimePolicy::default(),
                },
            },
            workflow_input: Value::Null,
            dependencies: BTreeMap::new(),
            resume_payload: None,
            services: NodeServiceContext {
                gateway_base_url: "http://gateway.test/v1".to_string(),
                default_model: "test".to_string(),
                memory_base_url: None,
                http_allowed_hosts: Vec::new(),
                max_http_response_bytes: 1024,
            },
        }
    }

    fn result(route: Option<&str>) -> NodeExecutionResult {
        NodeExecutionResult {
            schema: NODE_RESULT_SCHEMA.to_string(),
            output: Value::Null,
            route: route.map(str::to_string),
            suspension: None,
            metadata: Map::new(),
        }
    }

    fn test_database_url() -> Option<String> {
        std::env::var("A3S_WORKFLOW_TEST_DATABASE_URL").ok()
    }

    fn graph_config() -> GraphRuntimeConfig {
        GraphRuntimeConfig {
            runtime: RuntimeConfig {
                default_provider: "local".to_string(),
                invocation_base_url: "http://control.test".to_string(),
                node_artifact_uri: "file:///a3s-workflow-node".to_string(),
                node_artifact_digest: format!("sha256:{}", "a".repeat(64)),
                node_artifact_media_type: "application/vnd.a3s.workflow.node-runner.v1".to_string(),
                node_command: Vec::new(),
                default_cpu_millis: 500,
                default_memory_bytes: 256 * 1024 * 1024,
                default_pids: 128,
                default_timeout_ms: 120_000,
                output_max_bytes: 1024,
            },
            providers: BTreeMap::from([(
                "local".to_string(),
                RuntimeProviderConfig {
                    endpoint: "http://runtime.test".to_string(),
                    api_token: "runtime-token".to_string(),
                },
            )]),
            gateway: GatewayConfig {
                base_url: "http://gateway.test/v1".to_string(),
                api_key_reference: "env://GATEWAY_KEY".to_string(),
                default_model: "test-model".to_string(),
            },
            memory: MemoryConfig {
                base_url: "http://memory.test/api/v1".to_string(),
                api_key_reference: "env://MEMORY_KEY".to_string(),
            },
            http_allowed_hosts: vec!["example.test".to_string()],
            max_http_response_bytes: 4096,
        }
    }

    async fn replay_runtime() -> Option<GraphRuntime> {
        let database_url = test_database_url()?;
        let executions = Arc::new(
            super::super::node_execution_store::PostgresNodeExecutionStore::connect(
                &database_url,
                2,
            )
            .await
            .expect("connect execution store"),
        );
        Some(GraphRuntime::new(graph_config(), executions).expect("graph runtime"))
    }

    fn definition(
        id: &str,
        nodes: Vec<WorkflowNode>,
        edges: Vec<WorkflowEdge>,
    ) -> WorkflowDefinition {
        let now = Utc::now();
        WorkflowDefinition {
            id: id.to_string(),
            name: format!("{id} workflow"),
            description: "Runtime replay coverage".to_string(),
            version: 1,
            nodes,
            edges,
            created_at: now,
            updated_at: now,
        }
    }

    fn node(id: &str, kind: NodeKind) -> WorkflowNode {
        let mut value = invocation(kind).node;
        value.id = id.to_string();
        value.data.label = id.to_string();
        value
    }

    fn edge(id: &str, source: &str, target: &str, handle: Option<&str>) -> WorkflowEdge {
        WorkflowEdge {
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            source_handle: handle.map(str::to_string),
        }
    }

    fn envelope(sequence: u64, event: FlowEvent) -> FlowEventEnvelope {
        FlowEventEnvelope {
            run_id: "replay-run".to_string(),
            sequence,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
        }
    }

    fn completed(
        sequence: u64,
        node_id: &str,
        phase: NodeExecutionPhase,
        value: NodeExecutionResult,
    ) -> FlowEventEnvelope {
        envelope(
            sequence,
            FlowEvent::StepCompleted {
                step_id: GraphRuntime::step_id(node_id, phase),
                output: serde_json::to_value(value).expect("serialize node result"),
            },
        )
    }

    fn workflow_invocation(
        definition: WorkflowDefinition,
        history: Vec<FlowEventEnvelope>,
    ) -> WorkflowInvocation {
        let input = serde_json::to_value(GraphRunInput {
            definition: definition.clone(),
            input: json!({"name": "Runtime"}),
        })
        .expect("serialize graph input");
        WorkflowInvocation {
            run_id: "replay-run".to_string(),
            spec: WorkflowSpec::rust_embedded(
                format!("workflow.{}", definition.id),
                "1",
                "test",
                "run",
            ),
            input,
            history,
        }
    }

    fn scheduled_ids(command: RuntimeCommand) -> Vec<String> {
        match command {
            RuntimeCommand::ScheduleSteps { steps } => {
                steps.into_iter().map(|step| step.step_id).collect()
            }
            other => panic!("expected scheduled steps, got {other:?}"),
        }
    }

    #[derive(Clone)]
    struct StubRuntimeClient {
        output_uri: String,
        output: Vec<u8>,
        state: RuntimeUnitState,
    }

    impl StubRuntimeClient {
        fn unavailable<T>() -> RuntimeResult<T> {
            Err(RuntimeError::ProviderUnavailable(
                "unused test operation".to_string(),
            ))
        }
    }

    #[async_trait]
    impl RuntimeClient for StubRuntimeClient {
        async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
            Self::unavailable()
        }

        async fn apply(&self, request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
            let succeeded = self.state == RuntimeUnitState::Succeeded;
            Ok(RuntimeObservation {
                schema: RuntimeObservation::SCHEMA.to_string(),
                unit_id: request.spec.unit_id.clone(),
                generation: request.spec.generation,
                spec_digest: request.spec.digest().map_err(RuntimeError::Protocol)?,
                class: request.spec.class,
                state: self.state,
                provider_resource_id: Some("test-resource".to_string()),
                provider_build: Some("coverage-runtime".to_string()),
                observed_at_ms: now_ms(),
                started_at_ms: Some(now_ms()),
                finished_at_ms: Some(now_ms()),
                health: None,
                outputs: if succeeded {
                    vec![RuntimeOutputArtifact {
                        name: "result".to_string(),
                        artifact: ArtifactRef {
                            uri: self.output_uri.clone(),
                            digest: digest(&self.output),
                            media_type: NODE_RESULT_MEDIA_TYPE.to_string(),
                        },
                        size_bytes: self.output.len() as u64,
                    }]
                } else {
                    Vec::new()
                },
                usage: None,
                evidence: None,
                provider_attestation: None,
                failure: (self.state == RuntimeUnitState::Failed).then(|| RuntimeFailure {
                    code: "node_failed".to_string(),
                    message: "stub Runtime failed".to_string(),
                    retryable: false,
                }),
            })
        }

        async fn inspect(&self, _unit_id: &str) -> RuntimeResult<RuntimeInspection> {
            Self::unavailable()
        }

        async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
            Self::unavailable()
        }

        async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
            Self::unavailable()
        }

        async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
            Self::unavailable()
        }

        async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
            Self::unavailable()
        }
    }

    async fn artifact_server(
        routes: BTreeMap<String, (u16, Vec<u8>)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind artifact server");
        let address = listener.local_addr().expect("artifact server address");
        let routes = Arc::new(routes);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let routes = Arc::clone(&routes);
                tokio::spawn(async move {
                    let mut request = vec![0_u8; 4096];
                    let Ok(read) = stream.read(&mut request).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&request[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, body) = routes
                        .get(path)
                        .cloned()
                        .unwrap_or_else(|| (404, b"missing".to_vec()));
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let response = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    if stream.write_all(response.as_bytes()).await.is_ok() {
                        let _ = stream.write_all(&body).await;
                    }
                });
            }
        });
        (format!("http://{address}"), task)
    }

    fn runtime_with_client(
        config: GraphRuntimeConfig,
        executions: SharedNodeExecutionStore,
        client: Arc<dyn RuntimeClient>,
    ) -> GraphRuntime {
        GraphRuntime {
            config: Arc::new(config),
            providers: Arc::new(BTreeMap::from([("local".to_string(), client)])),
            executions,
            client: reqwest::Client::new(),
        }
    }

    #[tokio::test]
    async fn workflow_replay_schedules_runtime_nodes_routes_and_approvals() {
        let Some(runtime) = replay_runtime().await else {
            return;
        };

        let linear = definition(
            "linear-replay",
            vec![
                node("start", NodeKind::Start),
                node("output", NodeKind::Output),
            ],
            vec![edge("start-output", "start", "output", None)],
        );
        assert_eq!(
            scheduled_ids(
                runtime
                    .run_workflow(workflow_invocation(linear.clone(), Vec::new()))
                    .await
                    .expect("schedule start")
            ),
            vec!["node:start:execute"]
        );
        let start_done = completed(
            1,
            "start",
            NodeExecutionPhase::Execute,
            NodeExecutionResult::completed(json!({"name": "Runtime"})),
        );
        assert_eq!(
            scheduled_ids(
                runtime
                    .run_workflow(workflow_invocation(
                        linear.clone(),
                        vec![start_done.clone()],
                    ))
                    .await
                    .expect("schedule output")
            ),
            vec!["node:output:execute"]
        );
        let output = json!({"message": "complete"});
        let command = runtime
            .run_workflow(workflow_invocation(
                linear,
                vec![
                    start_done.clone(),
                    completed(
                        2,
                        "output",
                        NodeExecutionPhase::Execute,
                        NodeExecutionResult::completed(output.clone()),
                    ),
                ],
            ))
            .await
            .expect("complete graph");
        assert_eq!(command, RuntimeCommand::Complete { output });

        let routed = definition(
            "router-replay",
            vec![
                node("start", NodeKind::Start),
                node("router", NodeKind::Router),
                node("left", NodeKind::Template),
                node("right", NodeKind::Template),
                node("output", NodeKind::Output),
            ],
            vec![
                edge("start-router", "start", "router", None),
                edge("router-left", "router", "left", Some("left")),
                edge("router-right", "router", "right", Some("right")),
                edge("left-output", "left", "output", None),
                edge("right-output", "right", "output", None),
            ],
        );
        assert_eq!(
            scheduled_ids(
                runtime
                    .run_workflow(workflow_invocation(
                        routed.clone(),
                        vec![start_done.clone()],
                    ))
                    .await
                    .expect("schedule router")
            ),
            vec!["node:router:execute"]
        );
        let mut selected_left = result(Some("left"));
        selected_left.output = json!({"selected": "left"});
        let route_done = completed(
            2,
            "router",
            NodeExecutionPhase::Execute,
            selected_left.clone(),
        );
        assert_eq!(
            scheduled_ids(
                runtime
                    .run_workflow(workflow_invocation(
                        routed.clone(),
                        vec![start_done.clone(), route_done.clone()],
                    ))
                    .await
                    .expect("schedule selected branch")
            ),
            vec!["node:left:execute"]
        );
        assert_eq!(
            scheduled_ids(
                runtime
                    .run_workflow(workflow_invocation(
                        routed.clone(),
                        vec![
                            start_done.clone(),
                            route_done,
                            completed(
                                3,
                                "left",
                                NodeExecutionPhase::Execute,
                                NodeExecutionResult::completed(json!({"branch": "left"})),
                            ),
                        ],
                    ))
                    .await
                    .expect("join active branch")
            ),
            vec!["node:output:execute"]
        );
        let unknown_route = runtime
            .run_workflow(workflow_invocation(
                routed,
                vec![
                    start_done.clone(),
                    completed(
                        2,
                        "router",
                        NodeExecutionPhase::Execute,
                        result(Some("missing")),
                    ),
                ],
            ))
            .await
            .expect_err("reject unknown route");
        assert!(unknown_route.to_string().contains("selected unknown route"));

        let approval = definition(
            "approval-replay",
            vec![
                node("start", NodeKind::Start),
                node("approval", NodeKind::Approval),
                node("output", NodeKind::Output),
            ],
            vec![
                edge("start-approval", "start", "approval", None),
                edge("approval-output", "approval", "output", None),
            ],
        );
        let mut suspension = result(None);
        suspension.suspension = Some(NodeSuspension::HumanApproval {
            message: "Approve deployment".to_string(),
            details: json!({"environment": "production"}),
        });
        let approval_done = completed(2, "approval", NodeExecutionPhase::Execute, suspension);
        let command = runtime
            .run_workflow(workflow_invocation(
                approval.clone(),
                vec![start_done.clone(), approval_done.clone()],
            ))
            .await
            .expect("create approval hook");
        match command {
            RuntimeCommand::CreateHook {
                hook_id, metadata, ..
            } => {
                assert_eq!(hook_id, "approval");
                assert_eq!(metadata["kind"], "human_approval");
                assert_eq!(metadata["labels"]["node_id"], "approval");
            }
            other => panic!("expected approval hook, got {other:?}"),
        }

        let disposed = runtime
            .run_workflow(workflow_invocation(
                approval.clone(),
                vec![
                    start_done.clone(),
                    envelope(
                        2,
                        FlowEvent::HookDisposed {
                            hook_id: "approval".to_string(),
                        },
                    ),
                ],
            ))
            .await
            .expect("disposed approval becomes workflow failure");
        assert!(matches!(
            disposed,
            RuntimeCommand::Fail { error } if error.contains("was disposed")
        ));

        let received = envelope(
            3,
            FlowEvent::HookReceived {
                hook_id: "approval".to_string(),
                payload: json!({"approved": true}),
            },
        );
        assert_eq!(
            scheduled_ids(
                runtime
                    .run_workflow(workflow_invocation(
                        approval.clone(),
                        vec![start_done.clone(), approval_done.clone(), received.clone()],
                    ))
                    .await
                    .expect("schedule approval resume")
            ),
            vec!["node:approval:resume"]
        );
        assert_eq!(
            scheduled_ids(
                runtime
                    .run_workflow(workflow_invocation(
                        approval.clone(),
                        vec![
                            start_done.clone(),
                            approval_done,
                            received,
                            completed(
                                4,
                                "approval",
                                NodeExecutionPhase::Resume,
                                NodeExecutionResult::completed(json!({"approved": true})),
                            ),
                        ],
                    ))
                    .await
                    .expect("schedule output after approval")
            ),
            vec!["node:output:execute"]
        );
        let missing_suspension = runtime
            .run_workflow(workflow_invocation(
                approval,
                vec![
                    start_done,
                    completed(
                        2,
                        "approval",
                        NodeExecutionPhase::Execute,
                        NodeExecutionResult::completed(Value::Null),
                    ),
                ],
            ))
            .await
            .expect_err("approval must request suspension");
        assert!(missing_suspension
            .to_string()
            .contains("completed without requesting approval"));
    }

    #[tokio::test]
    async fn runtime_spec_preserves_policy_secrets_and_service_context() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let executions = Arc::new(
            super::super::node_execution_store::PostgresNodeExecutionStore::connect(
                &database_url,
                2,
            )
            .await
            .expect("connect execution store"),
        );
        let runtime = GraphRuntime::new(graph_config(), executions.clone()).expect("graph runtime");
        let mut value = invocation(NodeKind::Llm);
        value.run_id = format!("runtime-spec-{}", Uuid::new_v4());
        value.step_id = "node:llm:execute".to_string();
        value.attempt = 3;
        value.node.data.runtime = NodeRuntimePolicy {
            provider: Some("local".to_string()),
            pool: Some("gpu".to_string()),
            cpu_millis: Some(2_000),
            memory_bytes: Some(8 * 1024 * 1024 * 1024),
            pids: Some(512),
            timeout_ms: Some(45_000),
            isolation: Some(NodeIsolation::Confidential),
            network: Some(NodeNetworkMode::Outbound),
            secrets: vec![
                NodeSecretReference {
                    name: "custom-api-key".to_string(),
                    reference: "env://CUSTOM_API_KEY".to_string(),
                    target: NodeSecretTarget::Environment {
                        variable: "CUSTOM_API_KEY".to_string(),
                    },
                },
                NodeSecretReference {
                    name: "client-certificate".to_string(),
                    reference: "vault://workflow/client-certificate".to_string(),
                    target: NodeSecretTarget::File {
                        path: "/run/secrets/client.pem".to_string(),
                        mode: 0o400,
                    },
                },
            ],
        };
        let prepared = executions
            .prepare(&value, "local-gpu", Some("gpu"))
            .await
            .expect("prepare invocation");
        let spec = runtime
            .runtime_spec(&value, &prepared)
            .expect("runtime unit spec");

        assert_eq!(runtime.provider_selector(&value.node), "local-gpu");
        assert_eq!(
            spec.unit_id,
            format!("workflow/{}/node/llm/execute", value.run_id)
        );
        assert_eq!(spec.generation, 3);
        assert_eq!(spec.resources.cpu_millis, 2_000);
        assert_eq!(spec.resources.memory_bytes, 8 * 1024 * 1024 * 1024);
        assert_eq!(spec.resources.pids, 512);
        assert_eq!(spec.resources.execution_timeout_ms, Some(45_000));
        assert_eq!(spec.resources.ephemeral_storage_bytes, Some(4_096));
        assert_eq!(spec.isolation, IsolationLevel::Confidential);
        assert_eq!(spec.network.mode, NetworkMode::Outbound);
        assert_eq!(spec.process.environment["A3S_RUNTIME_POOL"], "gpu");
        assert_eq!(spec.process.environment["A3S_WORKFLOW_NODE_KIND"], "llm");
        assert_eq!(spec.mounts.len(), 1);
        match &spec.mounts[0].source {
            RuntimeMountSource::Artifact { artifact } => {
                assert_eq!(artifact.digest, prepared.invocation_digest);
                assert!(artifact.uri.contains(&prepared.execution_id));
                assert!(artifact.uri.contains(&prepared.token));
            }
            other => panic!("expected invocation artifact mount, got {other:?}"),
        }
        assert_eq!(spec.secrets.len(), 3);
        assert!(spec
            .secrets
            .iter()
            .any(|secret| secret.name == "a3s-gateway-api-key"));
        assert!(spec.secrets.iter().any(|secret| {
            secret.name == "client-certificate"
                && matches!(
                    &secret.target,
                    SecretTarget::File { path, mode }
                        if path == "/run/secrets/client.pem" && *mode == 0o400
                )
        }));
        assert_eq!(spec.outputs[0].max_bytes, 1_024);
        assert_eq!(
            spec.semantics_profile_digest,
            Some(semantics_digest(
                &graph_config().runtime.node_artifact_digest
            ))
        );

        let services = runtime.services();
        assert_eq!(services.gateway_base_url, "http://gateway.test/v1");
        assert_eq!(
            services.memory_base_url.as_deref(),
            Some("http://memory.test/api/v1")
        );
        assert_eq!(services.http_allowed_hosts, vec!["example.test"]);

        let mut memory = invocation(NodeKind::Memory);
        assert!(runtime
            .runtime_secrets(&memory)
            .expect("memory secret")
            .iter()
            .any(|secret| secret.name == "a3s-memory-api-key"));

        memory.node.kind = NodeKind::Llm;
        memory.node.data.runtime.secrets.push(NodeSecretReference {
            name: "a3s-gateway-api-key".to_string(),
            reference: "env://DUPLICATE".to_string(),
            target: NodeSecretTarget::Environment {
                variable: "DUPLICATE".to_string(),
            },
        });
        assert!(runtime
            .runtime_secrets(&memory)
            .expect_err("duplicate secret name")
            .to_string()
            .contains("duplicate Runtime secret names"));

        let mut invalid = graph_config();
        invalid
            .providers
            .get_mut("local")
            .expect("provider")
            .endpoint = "://invalid".to_string();
        assert!(GraphRuntime::new(invalid, executions).is_err());
    }

    #[tokio::test]
    async fn runtime_dispatch_records_evidence_and_surfaces_failures() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let executions = Arc::new(
            super::super::node_execution_store::PostgresNodeExecutionStore::connect(
                &database_url,
                2,
            )
            .await
            .expect("connect execution store"),
        );
        let successful_result = NodeExecutionResult::completed(json!({"rendered": "hello"}));
        let output = serde_json::to_vec(&successful_result).expect("serialize result");
        let (base_url, server) = artifact_server(BTreeMap::from([(
            "/result".to_string(),
            (200, output.clone()),
        )]))
        .await;
        let runtime = runtime_with_client(
            graph_config(),
            executions.clone(),
            Arc::new(StubRuntimeClient {
                output_uri: format!("{base_url}/result"),
                output: output.clone(),
                state: RuntimeUnitState::Succeeded,
            }),
        );
        let run_id = format!("runtime-dispatch-{}", Uuid::new_v4());
        let step_id = "node:template:execute".to_string();
        let step_input = GraphStepInput {
            workflow_id: "workflow".to_string(),
            workflow_version: 7,
            phase: NodeExecutionPhase::Execute,
            node: node("template", NodeKind::Template),
            workflow_input: json!({"name": "hello"}),
            dependencies: BTreeMap::from([("start".to_string(), json!({"ready": true}))]),
            resume_payload: None,
        };
        let step = StepInvocation {
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            step_name: "template:execute".to_string(),
            input: serde_json::to_value(step_input).expect("serialize step input"),
            history: vec![envelope(
                1,
                FlowEvent::StepStarted {
                    step_id: step_id.clone(),
                    attempt: 2,
                },
            )],
        };
        let result_value = runtime.run_step(step).await.expect("dispatch runtime step");
        assert_eq!(
            serde_json::from_value::<NodeExecutionResult>(result_value).expect("decode result"),
            successful_result
        );
        let evidence = executions
            .list_for_run(&run_id)
            .await
            .expect("list Runtime evidence");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].attempt, 2);
        assert_eq!(evidence[0].state, "succeeded");
        assert_eq!(evidence[0].provider_id, "local");
        assert!(evidence[0]
            .unit_id
            .as_deref()
            .is_some_and(|id| id.contains(&run_id)));
        assert!(evidence[0].observation.is_some());

        let missing_attempt = runtime
            .run_step(StepInvocation {
                run_id: format!("missing-attempt-{}", Uuid::new_v4()),
                step_id: step_id.clone(),
                step_name: "template:execute".to_string(),
                input: json!({
                    "workflowId": "workflow",
                    "workflowVersion": 1,
                    "phase": "execute",
                    "node": node("template", NodeKind::Template),
                    "workflowInput": null,
                    "dependencies": {}
                }),
                history: Vec::new(),
            })
            .await
            .expect_err("durable attempt required");
        assert!(missing_attempt
            .to_string()
            .contains("has no durable attempt identity"));

        let mut invalid = invocation(NodeKind::Template);
        invalid.schema = "unsupported".to_string();
        assert!(runtime
            .dispatch(invalid)
            .await
            .expect_err("invalid invocation")
            .to_string()
            .contains("invalid node invocation"));

        let mut unconfigured = invocation(NodeKind::Template);
        unconfigured.run_id = format!("unconfigured-{}", Uuid::new_v4());
        unconfigured.node.data.runtime.provider = Some("missing".to_string());
        assert!(runtime
            .dispatch(unconfigured)
            .await
            .expect_err("unconfigured provider")
            .to_string()
            .contains("unconfigured Runtime provider"));

        for (state, expected) in [
            (RuntimeUnitState::Failed, "node_failed: stub Runtime failed"),
            (RuntimeUnitState::Stopped, "Runtime unit ended in Stopped"),
        ] {
            let failed_runtime = runtime_with_client(
                graph_config(),
                executions.clone(),
                Arc::new(StubRuntimeClient {
                    output_uri: format!("{base_url}/result"),
                    output: output.clone(),
                    state,
                }),
            );
            let mut failed = invocation(NodeKind::Template);
            failed.run_id = format!("runtime-{state:?}-{}", Uuid::new_v4());
            let error = failed_runtime
                .dispatch(failed)
                .await
                .expect_err("terminal Runtime failure");
            assert!(error.to_string().contains(expected));
        }
        server.abort();
    }

    #[tokio::test]
    async fn output_artifacts_reject_unsafe_or_corrupt_responses() {
        let Some(runtime) = replay_runtime().await else {
            return;
        };
        let valid = b"valid artifact".to_vec();
        let too_large = vec![
            b'x';
            usize::try_from(runtime.config.runtime.output_max_bytes + 1)
                .expect("test output limit fits usize")
        ];
        let (base_url, server) = artifact_server(BTreeMap::from([
            ("/ok".to_string(), (200, valid.clone())),
            ("/status".to_string(), (500, b"provider error".to_vec())),
            ("/large".to_string(), (200, too_large.clone())),
        ]))
        .await;

        assert_eq!(
            runtime
                .fetch_output(
                    &format!("{base_url}/ok"),
                    valid.len() as u64,
                    &digest(&valid)
                )
                .await
                .expect("fetch valid artifact"),
            valid
        );
        assert!(runtime
            .fetch_output(
                &format!("{base_url}/ok"),
                runtime.config.runtime.output_max_bytes + 1,
                &digest(b"valid artifact"),
            )
            .await
            .expect_err("reject reported oversize")
            .to_string()
            .contains("exceeds configured output limit"));
        assert!(runtime
            .fetch_output("not a URI", 1, &digest(b"x"))
            .await
            .expect_err("reject malformed URI")
            .to_string()
            .contains("invalid output artifact URI"));
        assert!(runtime
            .fetch_output("file:///tmp/result.json", 1, &digest(b"x"))
            .await
            .expect_err("reject local artifact URI")
            .to_string()
            .contains("must use http:// or https://"));
        assert!(runtime
            .fetch_output(
                &format!("{base_url}/status"),
                14,
                &digest(b"provider error")
            )
            .await
            .expect_err("reject provider status")
            .to_string()
            .contains("returned 500"));
        assert!(runtime
            .fetch_output(&format!("{base_url}/large"), 1, &digest(&too_large))
            .await
            .expect_err("reject actual oversize")
            .to_string()
            .contains("exceeds configured output limit"));
        assert!(runtime
            .fetch_output(&format!("{base_url}/ok"), 1, &digest(b"wrong"))
            .await
            .expect_err("reject digest mismatch")
            .to_string()
            .contains("digest mismatch"));
        server.abort();
    }

    #[test]
    fn semantics_profile_binds_protocol_and_runner_artifact() {
        let first = semantics_digest(&format!("sha256:{}", "a".repeat(64)));
        let second = semantics_digest(&format!("sha256:{}", "b".repeat(64)));

        assert_ne!(first, second);
        assert!(first.starts_with("sha256:"));
        assert_eq!(first.len(), 71);
    }

    #[test]
    fn phase_names_are_stable() {
        assert_eq!(phase_name(NodeExecutionPhase::Execute), "execute");
        assert_eq!(phase_name(NodeExecutionPhase::Resume), "resume");
    }

    #[test]
    fn every_node_kind_is_addressable_to_an_independently_scalable_runtime_pool() {
        for kind in [
            NodeKind::Start,
            NodeKind::Template,
            NodeKind::Llm,
            NodeKind::Agent,
            NodeKind::Tool,
            NodeKind::Router,
            NodeKind::Memory,
            NodeKind::Http,
            NodeKind::Approval,
            NodeKind::Output,
        ] {
            let mut node = invocation(kind).node;
            assert_eq!(runtime_selector("local", &node), "local");

            node.data.runtime.provider = Some("production".to_string());
            node.data.runtime.pool = Some(format!("{}-pool", kind.as_str()));
            assert_eq!(
                runtime_selector("local", &node),
                format!("production-{}-pool", kind.as_str())
            );
        }

        let mut node = invocation(NodeKind::Template).node;
        node.data.runtime.pool = Some("cpu".to_string());
        assert_eq!(runtime_selector("local", &node), "local-cpu");
    }

    #[test]
    fn network_defaults_follow_node_capabilities_and_allow_overrides() {
        for kind in [
            NodeKind::Start,
            NodeKind::Template,
            NodeKind::Router,
            NodeKind::Approval,
            NodeKind::Output,
        ] {
            assert_eq!(network_mode(&invocation(kind)), NetworkMode::None);
        }
        for kind in [
            NodeKind::Llm,
            NodeKind::Agent,
            NodeKind::Tool,
            NodeKind::Memory,
            NodeKind::Http,
        ] {
            assert_eq!(network_mode(&invocation(kind)), NetworkMode::Outbound);
        }

        let mut value = invocation(NodeKind::Http);
        value.node.data.runtime.network = Some(NodeNetworkMode::None);
        assert_eq!(network_mode(&value), NetworkMode::None);

        let mut value = invocation(NodeKind::Start);
        value.node.data.runtime.network = Some(NodeNetworkMode::Outbound);
        assert_eq!(network_mode(&value), NetworkMode::Outbound);
    }

    #[test]
    fn isolation_defaults_to_process_and_maps_every_policy() {
        let mut value = invocation(NodeKind::Template);
        assert_eq!(isolation_level(&value), IsolationLevel::Process);

        for (policy, expected) in [
            (NodeIsolation::Process, IsolationLevel::Process),
            (NodeIsolation::Container, IsolationLevel::Container),
            (NodeIsolation::Sandbox, IsolationLevel::Sandbox),
            (NodeIsolation::Confidential, IsolationLevel::Confidential),
        ] {
            value.node.data.runtime.isolation = Some(policy);
            assert_eq!(isolation_level(&value), expected);
        }
    }

    #[test]
    fn non_pending_nodes_cannot_return_suspensions() {
        let node = invocation(NodeKind::Template).node;
        ensure_no_suspension(&node, &result(None)).expect("completed node");

        let mut suspended = result(None);
        suspended.suspension = Some(NodeSuspension::HumanApproval {
            message: "wait".to_string(),
            details: Value::Null,
        });
        let error = ensure_no_suspension(&node, &suspended).expect_err("suspension must fail");
        assert!(error.to_string().contains("unexpected suspension"));
    }

    #[test]
    fn router_result_must_select_a_configured_outgoing_handle() {
        let node = invocation(NodeKind::Router).node;
        let edges = vec![WorkflowEdge {
            id: "route".to_string(),
            source: node.id.clone(),
            target: "output".to_string(),
            source_handle: Some("selected".to_string()),
        }];

        validate_route(&node, &result(Some("selected")), &edges).expect("known route");
        assert!(validate_route(&node, &result(None), &edges)
            .expect_err("missing route")
            .to_string()
            .contains("returned no route"));
        assert!(validate_route(&node, &result(Some("unknown")), &edges)
            .expect_err("unknown route")
            .to_string()
            .contains("selected unknown route"));
    }
}
