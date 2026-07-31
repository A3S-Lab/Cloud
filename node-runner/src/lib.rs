use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use a3s_workflow_protocol::{
    NodeExecutionPhase, NodeExecutionResult, NodeInvocation, NodeKind, NodeSuspension,
};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Method, RequestBuilder};
use serde_json::{json, Map, Value};
use url::Url;

#[derive(Clone)]
pub struct NodeExecutor {
    client: Client,
}

impl NodeExecutor {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("failed to build node HTTP client")?;
        Ok(Self { client })
    }

    pub async fn execute(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        invocation.validate().map_err(anyhow::Error::msg)?;
        let mut result = match invocation.node.kind {
            NodeKind::Start => NodeExecutionResult::completed(invocation.workflow_input.clone()),
            NodeKind::Template => self.execute_template(invocation)?,
            NodeKind::Llm => self.execute_llm(invocation).await?,
            NodeKind::Agent => self.execute_agent(invocation).await?,
            NodeKind::Tool | NodeKind::Http => self.execute_http(invocation).await?,
            NodeKind::Router => self.execute_router(invocation)?,
            NodeKind::Memory => self.execute_memory(invocation).await?,
            NodeKind::Approval => self.execute_approval(invocation)?,
            NodeKind::Output => self.execute_output(invocation)?,
        };
        result.metadata.insert(
            "nodeKind".to_string(),
            Value::String(invocation.node.kind.as_str().to_string()),
        );
        result.validate().map_err(anyhow::Error::msg)?;
        Ok(result)
    }

    fn rendered_config(&self, invocation: &NodeInvocation) -> Result<Value> {
        render_value(
            &invocation.node.data.config,
            &invocation.workflow_input,
            &invocation.dependencies,
        )
    }

    fn execute_template(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        if let Some(value) = config.get("value") {
            return Ok(NodeExecutionResult::completed(value.clone()));
        }
        let template = required_string(&config, "template", &invocation.node.id)?;
        Ok(NodeExecutionResult::completed(json!({ "text": template })))
    }

    async fn execute_llm(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let prompt = required_string(&config, "prompt", &invocation.node.id)?;
        let model = model_for(&config, invocation)?;
        let mut messages = Vec::new();
        if let Some(system) = nonempty_string(&config, "system") {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.push(json!({ "role": "user", "content": prompt }));

        let mut body = json!({ "model": model, "messages": messages });
        copy_optional_fields(
            &config,
            &mut body,
            &["temperature", "top_p", "max_tokens", "response_format"],
        );
        let payload = self.gateway_chat(invocation, &body).await?;
        let content = payload
            .pointer("/choices/0/message/content")
            .cloned()
            .ok_or_else(|| anyhow!("gateway response is missing choices[0].message.content"))?;
        Ok(NodeExecutionResult::completed(json!({
            "content": content,
            "model": model,
            "usage": payload.get("usage").cloned().unwrap_or(Value::Null)
        })))
    }

    async fn execute_agent(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let model = model_for(&config, invocation)?;
        let prompt = required_string(&config, "prompt", &invocation.node.id)?;
        let max_iterations = config
            .get("maxIterations")
            .and_then(Value::as_u64)
            .unwrap_or(6);
        if !(1..=16).contains(&max_iterations) {
            bail!("agent maxIterations must be between 1 and 16");
        }

        let tools = config
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let public_tools = tools
            .iter()
            .map(public_tool_definition)
            .collect::<Result<Vec<_>>>()?;
        let mut messages = Vec::new();
        if let Some(system) = nonempty_string(&config, "system") {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.push(json!({ "role": "user", "content": prompt }));
        let mut tool_results = Vec::new();

        for iteration in 1..=max_iterations {
            let mut request = json!({ "model": model, "messages": messages });
            if !public_tools.is_empty() {
                request["tools"] = Value::Array(public_tools.clone());
                request["tool_choice"] = Value::String("auto".to_string());
            }
            copy_optional_fields(
                &config,
                &mut request,
                &["temperature", "top_p", "max_tokens"],
            );
            let payload = self.gateway_chat(invocation, &request).await?;
            let message = payload
                .pointer("/choices/0/message")
                .cloned()
                .ok_or_else(|| anyhow!("gateway response is missing choices[0].message"))?;
            let calls = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            messages.push(message.clone());

            if calls.is_empty() {
                let content = message.get("content").cloned().unwrap_or(Value::Null);
                return Ok(NodeExecutionResult::completed(json!({
                    "content": content,
                    "model": model,
                    "iterations": iteration,
                    "toolResults": tool_results,
                    "usage": payload.get("usage").cloned().unwrap_or(Value::Null)
                })));
            }

            for call in calls {
                let call_id = required_json_string(&call, "id", "agent tool call")?;
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("agent tool call is missing function.name"))?;
                let arguments = call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                let arguments: Value = serde_json::from_str(arguments)
                    .with_context(|| format!("tool {name} returned invalid JSON arguments"))?;
                let definition = tools
                    .iter()
                    .find(|tool| tool_name(tool) == Some(name))
                    .ok_or_else(|| anyhow!("agent requested unknown tool {name}"))?;
                let output = self
                    .call_tool_definition(invocation, definition, &arguments)
                    .await?;
                tool_results.push(json!({ "name": name, "output": output }));
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": serde_json::to_string(&output)?
                }));
            }
        }

        bail!("agent exhausted maxIterations without producing a final response")
    }

    async fn execute_http(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let result = self.call_http_config(invocation, &config).await?;
        Ok(NodeExecutionResult::completed(result))
    }

    fn execute_router(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let cases = config
            .get("routes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("router node {} requires config.routes", invocation.node.id))?;
        for case in cases {
            let when = case
                .get("when")
                .and_then(Value::as_object)
                .ok_or_else(|| anyhow!("router route requires a when object"))?;
            let actual = when.get("value").unwrap_or(&Value::Null);
            let expected = when.get("equals").unwrap_or(&Value::Bool(true));
            if actual == expected {
                let route = required_json_string(case, "route", "router route")?;
                let mut result = NodeExecutionResult::completed(output_value(
                    invocation.dependencies.clone(),
                    &invocation.workflow_input,
                ));
                result.route = Some(route.to_string());
                return Ok(result);
            }
        }
        let route = required_string(&config, "default", &invocation.node.id)?;
        let mut result = NodeExecutionResult::completed(output_value(
            invocation.dependencies.clone(),
            &invocation.workflow_input,
        ));
        result.route = Some(route.to_string());
        Ok(result)
    }

    async fn execute_memory(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let base_url = invocation
            .services
            .memory_base_url
            .as_deref()
            .ok_or_else(|| anyhow!("memory node requires services.memoryBaseUrl"))?;
        let operation = config
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("search");
        let endpoint = match operation {
            "store" | "search" => {
                format!("{}/memories:{}", base_url.trim_end_matches('/'), operation)
            }
            "retrieve" | "delete" => {
                let id = required_string(&config, "id", &invocation.node.id)?;
                format!("{}/memories/{id}", base_url.trim_end_matches('/'))
            }
            _ => bail!("unsupported memory operation {operation}"),
        };
        let method = if operation == "retrieve" {
            Method::GET
        } else if operation == "delete" {
            Method::DELETE
        } else {
            Method::POST
        };
        let mut request = self.client.request(method, endpoint);
        if matches!(operation, "store" | "search") {
            request = request.json(&config);
        }
        if let Ok(token) = std::env::var("A3S_MEMORY_API_KEY") {
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
        }
        let response = request.send().await.context("A3S Memory request failed")?;
        let status = response.status();
        let body = limited_response(response, invocation.services.max_http_response_bytes).await?;
        if !status.is_success() {
            bail!("A3S Memory returned {status}: {}", body_text(&body));
        }
        let output = serde_json::from_slice(&body).context("A3S Memory returned invalid JSON")?;
        Ok(NodeExecutionResult::completed(output))
    }

    fn execute_approval(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        if invocation.phase == NodeExecutionPhase::Resume {
            return Ok(NodeExecutionResult::completed(
                invocation.resume_payload.clone().unwrap_or(Value::Null),
            ));
        }
        let config = self.rendered_config(invocation)?;
        let message = config
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(&invocation.node.data.label)
            .trim()
            .to_string();
        if message.is_empty() {
            bail!("approval message must not be empty");
        }
        let mut result = NodeExecutionResult::completed(Value::Null);
        result.suspension = Some(NodeSuspension::HumanApproval {
            message,
            details: config.get("details").cloned().unwrap_or(Value::Null),
        });
        Ok(result)
    }

    fn execute_output(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let output = config.get("value").cloned().unwrap_or_else(|| {
            output_value(invocation.dependencies.clone(), &invocation.workflow_input)
        });
        Ok(NodeExecutionResult::completed(output))
    }

    async fn gateway_chat(&self, invocation: &NodeInvocation, body: &Value) -> Result<Value> {
        let endpoint = format!(
            "{}/chat/completions",
            invocation.services.gateway_base_url.trim_end_matches('/')
        );
        let mut request = self.client.post(endpoint).json(body);
        if let Ok(api_key) = std::env::var("A3S_GATEWAY_API_KEY") {
            if !api_key.is_empty() {
                request = request.bearer_auth(api_key);
            }
        }
        let response = request.send().await.context("A3S Gateway request failed")?;
        let status = response.status();
        let body = limited_response(response, invocation.services.max_http_response_bytes).await?;
        if !status.is_success() {
            bail!("A3S Gateway returned {status}: {}", body_text(&body));
        }
        serde_json::from_slice(&body).context("A3S Gateway returned invalid JSON")
    }

    async fn call_tool_definition(
        &self,
        invocation: &NodeInvocation,
        definition: &Value,
        arguments: &Value,
    ) -> Result<Value> {
        let endpoint = required_json_string(definition, "endpoint", "agent tool")?;
        let config = json!({
            "url": endpoint,
            "method": definition.get("method").and_then(Value::as_str).unwrap_or("POST"),
            "headers": definition.get("headers").cloned().unwrap_or_else(|| json!({})),
            "headerEnv": definition.get("headerEnv").cloned().unwrap_or_else(|| json!({})),
            "body": arguments
        });
        self.call_http_config(invocation, &config).await
    }

    async fn call_http_config(&self, invocation: &NodeInvocation, config: &Value) -> Result<Value> {
        let raw_url = config
            .get("url")
            .or_else(|| config.get("endpoint"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("node {} requires config.url", invocation.node.id))?;
        let url = Url::parse(raw_url).context("invalid HTTP node URL")?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!("HTTP nodes only support http:// and https:// URLs");
        }
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("HTTP node URL has no host"))?;
        let allowed = invocation
            .services
            .http_allowed_hosts
            .iter()
            .map(|value| value.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if !host_is_allowed(host, &allowed) {
            bail!("HTTP node host {host} is not allow-listed");
        }
        let method = config
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .parse::<Method>()
            .context("invalid HTTP method")?;
        let mut request = self.client.request(method, url);
        request = apply_headers(request, config)?;
        if let Some(body) = config.get("body") {
            request = request.json(body);
        }
        let response = request.send().await.context("HTTP node request failed")?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = limited_response(response, invocation.services.max_http_response_bytes).await?;
        let body = if content_type.contains("application/json") {
            serde_json::from_slice(&body).context("HTTP node returned invalid JSON")?
        } else {
            Value::String(body_text(&body))
        };
        if !status.is_success() {
            bail!("HTTP node returned {status}: {body}");
        }
        Ok(json!({ "status": status.as_u16(), "body": body }))
    }
}

impl Default for NodeExecutor {
    fn default() -> Self {
        Self::new().expect("the static reqwest client configuration is valid")
    }
}

fn model_for<'a>(config: &'a Value, invocation: &'a NodeInvocation) -> Result<&'a str> {
    config
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            (!invocation.services.default_model.trim().is_empty())
                .then_some(invocation.services.default_model.as_str())
        })
        .ok_or_else(|| anyhow!("node {} requires a model", invocation.node.id))
}

fn nonempty_string<'a>(config: &'a Value, key: &str) -> Option<&'a str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn required_string<'a>(config: &'a Value, key: &str, node_id: &str) -> Result<&'a str> {
    nonempty_string(config, key)
        .ok_or_else(|| anyhow!("node {node_id} requires non-empty config.{key}"))
}

fn required_json_string<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("{context} requires a non-empty {key}"))
}

fn copy_optional_fields(source: &Value, target: &mut Value, fields: &[&str]) {
    for field in fields {
        if let Some(value) = source.get(*field) {
            target[*field] = value.clone();
        }
    }
}

fn public_tool_definition(tool: &Value) -> Result<Value> {
    let name = tool_name(tool).ok_or_else(|| anyhow!("agent tool requires function.name"))?;
    let description = tool
        .pointer("/function/description")
        .and_then(Value::as_str)
        .unwrap_or(name);
    let parameters = tool
        .pointer("/function/parameters")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    Ok(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    }))
}

fn tool_name(tool: &Value) -> Option<&str> {
    tool.pointer("/function/name").and_then(Value::as_str)
}

fn apply_headers(mut request: RequestBuilder, config: &Value) -> Result<RequestBuilder> {
    if let Some(headers) = config.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            let value = value
                .as_str()
                .ok_or_else(|| anyhow!("HTTP header {name} must be a string"))?;
            request = request.header(name, value);
        }
    }
    if let Some(headers) = config.get("headerEnv").and_then(Value::as_object) {
        for (name, variable) in headers {
            let variable = variable
                .as_str()
                .ok_or_else(|| anyhow!("headerEnv {name} must name an environment variable"))?;
            let value = std::env::var(variable)
                .with_context(|| format!("required secret environment {variable} is missing"))?;
            request = request.header(name, value);
        }
    }
    Ok(request)
}

fn host_is_allowed(host: &str, allowed: &BTreeSet<String>) -> bool {
    let host = host.to_ascii_lowercase();
    allowed.iter().any(|candidate| {
        candidate == &host
            || candidate
                .strip_prefix("*.")
                .is_some_and(|suffix| host.ends_with(&format!(".{suffix}")))
    })
}

async fn limited_response(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|content_length| content_length > limit as u64)
    {
        bail!("response exceeds configured limit of {limit} bytes");
    }
    let bytes = response.bytes().await.context("failed to read response")?;
    if bytes.len() > limit {
        bail!("response exceeds configured limit of {limit} bytes");
    }
    Ok(bytes.to_vec())
}

fn body_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

fn output_value(mut dependencies: BTreeMap<String, Value>, workflow_input: &Value) -> Value {
    match dependencies.len() {
        0 => workflow_input.clone(),
        1 => dependencies
            .pop_first()
            .map(|(_, value)| value)
            .unwrap_or(Value::Null),
        _ => Value::Object(dependencies.into_iter().collect()),
    }
}

fn render_value(
    value: &Value,
    workflow_input: &Value,
    dependencies: &BTreeMap<String, Value>,
) -> Result<Value> {
    match value {
        Value::String(value) => render_string(value, workflow_input, dependencies),
        Value::Array(values) => values
            .iter()
            .map(|value| render_value(value, workflow_input, dependencies))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                render_value(value, workflow_input, dependencies).map(|value| (key.clone(), value))
            })
            .collect::<Result<Map<_, _>>>()
            .map(Value::Object),
        value => Ok(value.clone()),
    }
}

fn render_string(
    source: &str,
    workflow_input: &Value,
    dependencies: &BTreeMap<String, Value>,
) -> Result<Value> {
    let trimmed = source.trim();
    if let Some(token) = whole_token(trimmed) {
        return lookup_token(token, workflow_input, dependencies).cloned();
    }
    let mut output = String::new();
    let mut remainder = source;
    while let Some(start) = remainder.find("{{") {
        output.push_str(&remainder[..start]);
        let token_source = &remainder[start + 2..];
        let end = token_source
            .find("}}")
            .ok_or_else(|| anyhow!("template contains an unclosed token"))?;
        let token = token_source[..end].trim();
        output.push_str(&scalar_text(lookup_token(
            token,
            workflow_input,
            dependencies,
        )?));
        remainder = &token_source[end + 2..];
    }
    output.push_str(remainder);
    Ok(Value::String(output))
}

fn whole_token(source: &str) -> Option<&str> {
    source
        .strip_prefix("{{")?
        .strip_suffix("}}")
        .map(str::trim)
        .filter(|token| !token.contains("}}"))
}

fn lookup_token<'a>(
    token: &str,
    workflow_input: &'a Value,
    dependencies: &'a BTreeMap<String, Value>,
) -> Result<&'a Value> {
    if token == "input" {
        return Ok(workflow_input);
    }
    if let Some(path) = token.strip_prefix("input.") {
        return lookup_path(workflow_input, path)
            .ok_or_else(|| anyhow!("template token {token} was not found"));
    }
    if let Some(path) = token.strip_prefix("steps.") {
        let (node_id, nested) = path.split_once('.').unwrap_or((path, ""));
        let value = dependencies
            .get(node_id)
            .ok_or_else(|| anyhow!("template dependency {node_id} is unavailable"))?;
        if nested.is_empty() {
            return Ok(value);
        }
        return lookup_path(value, nested)
            .ok_or_else(|| anyhow!("template token {token} was not found"));
    }
    bail!("unsupported template token {token}; use input.* or steps.<node>.*")
}

fn lookup_path<'a>(mut value: &'a Value, path: &str) -> Option<&'a Value> {
    for segment in path.split('.') {
        value = match value {
            Value::Object(object) => object.get(segment)?,
            Value::Array(values) => values.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_workflow_protocol::{
        NodeData, NodeRuntimePolicy, NodeServiceContext, Position, WorkflowNode,
        NODE_INVOCATION_SCHEMA,
    };

    fn invocation(kind: NodeKind, config: Value) -> NodeInvocation {
        NodeInvocation {
            schema: NODE_INVOCATION_SCHEMA.to_string(),
            run_id: "run-1".to_string(),
            step_id: format!("node:{}:execute", kind.as_str()),
            attempt: 1,
            workflow_id: "workflow-1".to_string(),
            workflow_version: 1,
            phase: NodeExecutionPhase::Execute,
            node: WorkflowNode {
                id: kind.as_str().to_string(),
                kind,
                position: Position { x: 0.0, y: 0.0 },
                data: NodeData {
                    label: kind.as_str().to_string(),
                    config,
                    runtime: NodeRuntimePolicy::default(),
                },
            },
            workflow_input: json!({ "name": "Ada" }),
            dependencies: BTreeMap::from([("draft".to_string(), json!({ "id": 7 }))]),
            resume_payload: None,
            services: NodeServiceContext {
                gateway_base_url: "http://gateway.test/v1".to_string(),
                default_model: "test-model".to_string(),
                memory_base_url: None,
                http_allowed_hosts: Vec::new(),
                max_http_response_bytes: 1024,
            },
        }
    }

    #[tokio::test]
    async fn template_preserves_typed_tokens() {
        let result = NodeExecutor::default()
            .execute(&invocation(
                NodeKind::Template,
                json!({ "value": { "name": "{{input.name}}", "id": "{{steps.draft.id}}" } }),
            ))
            .await
            .expect("template node");
        assert_eq!(result.output, json!({ "name": "Ada", "id": 7 }));
    }

    #[tokio::test]
    async fn router_returns_an_explicit_handle() {
        let result = NodeExecutor::default()
            .execute(&invocation(
                NodeKind::Router,
                json!({
                    "routes": [{ "when": { "value": "{{input.name}}", "equals": "Ada" }, "route": "known" }],
                    "default": "unknown"
                }),
            ))
            .await
            .expect("router node");
        assert_eq!(result.route.as_deref(), Some("known"));
    }

    #[tokio::test]
    async fn approval_suspends_then_resumes() {
        let executor = NodeExecutor::default();
        let initial = executor
            .execute(&invocation(
                NodeKind::Approval,
                json!({ "message": "Ship it?" }),
            ))
            .await
            .expect("approval request");
        assert!(initial.suspension.is_some());

        let mut resumed = invocation(NodeKind::Approval, json!({ "message": "Ship it?" }));
        resumed.phase = NodeExecutionPhase::Resume;
        resumed.resume_payload = Some(json!({ "approved": true }));
        let result = executor.execute(&resumed).await.expect("approval resume");
        assert_eq!(result.output, json!({ "approved": true }));
        assert!(result.suspension.is_none());
    }
}
