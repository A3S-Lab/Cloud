use std::sync::Arc;

use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, Module, ModuleRef,
    ProviderDefinition, Result,
};
use a3s_workflow_protocol::NODE_INVOCATION_MEDIA_TYPE;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::modules::workflow::application::{StartRunRequest, WorkflowService};
use crate::modules::workflow::domain::{WorkflowDraft, WorkflowError, WorkflowUpdate};

#[derive(Clone)]
pub struct WorkflowModule {
    service: Arc<WorkflowService>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelRunRequest {
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalRequest {
    #[serde(default)]
    payload: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeTypeDescriptor {
    kind: &'static str,
    label: &'static str,
    description: &'static str,
    default_config: Value,
}

impl WorkflowModule {
    pub fn new(service: Arc<WorkflowService>) -> Self {
        Self { service }
    }

    fn workflow_controller(service: Arc<WorkflowService>) -> Result<ControllerDefinition> {
        let list_service = Arc::clone(&service);
        let get_service = Arc::clone(&service);
        let create_service = Arc::clone(&service);
        let update_service = Arc::clone(&service);
        let delete_service = Arc::clone(&service);
        let run_service = Arc::clone(&service);

        ControllerDefinition::new("/api/v1/workflows")?
            .get_json("/", move |_| {
                let service = Arc::clone(&list_service);
                async move { service.list().await.map_err(map_error) }
            })?
            .get_json("/{id}", move |request| {
                let service = Arc::clone(&get_service);
                async move {
                    let id = required_param(&request, "id")?;
                    service.get(&id).await.map_err(map_error)
                }
            })?
            .post_json_with_status::<WorkflowDraft, _, _, _>("/", 201, move |draft| {
                let service = Arc::clone(&create_service);
                async move { service.create(draft).await.map_err(map_error) }
            })?
            .put("/{id}", move |request: BootRequest| {
                let service = Arc::clone(&update_service);
                async move {
                    let id = required_param(&request, "id")?;
                    let update = request.json::<WorkflowUpdate>()?;
                    let workflow = service.update(&id, update).await.map_err(map_error)?;
                    BootResponse::json(&workflow)
                }
            })?
            .delete("/{id}", move |request: BootRequest| {
                let service = Arc::clone(&delete_service);
                async move {
                    let id = required_param(&request, "id")?;
                    service.delete(&id).await.map_err(map_error)?;
                    Ok(BootResponse::no_content())
                }
            })?
            .post("/{id}/runs", move |request: BootRequest| {
                let service = Arc::clone(&run_service);
                async move {
                    let id = required_param(&request, "id")?;
                    let payload = request.json::<StartRunRequest>()?;
                    let run = service.start_run(&id, payload).await.map_err(map_error)?;
                    BootResponse::json_with_status(201, &run)
                }
            })
    }

    fn run_controller(service: Arc<WorkflowService>) -> Result<ControllerDefinition> {
        let list_service = Arc::clone(&service);
        let summary_service = Arc::clone(&service);
        let event_service = Arc::clone(&service);
        let get_service = Arc::clone(&service);
        let history_service = Arc::clone(&service);
        let execution_service = Arc::clone(&service);
        let cancel_service = Arc::clone(&service);
        let approval_service = Arc::clone(&service);

        ControllerDefinition::new("/api/v1/runs")?
            .get_json("/", move |request| {
                let service = Arc::clone(&list_service);
                async move {
                    service
                        .list_runs(request.query_param("workflowId"))
                        .await
                        .map_err(map_error)
                }
            })?
            .get_json("/summary", move |_| {
                let service = Arc::clone(&summary_service);
                async move { service.run_summary().await.map_err(map_error) }
            })?
            .get_json("/events", move |request| {
                let service = Arc::clone(&event_service);
                async move {
                    let limit = request
                        .optional_query_value_as::<usize>("limit")?
                        .unwrap_or(100);
                    service.recent_events(limit).await.map_err(map_error)
                }
            })?
            .get_json("/{id}", move |request| {
                let service = Arc::clone(&get_service);
                async move {
                    let id = required_param(&request, "id")?;
                    service.get_run(&id).await.map_err(map_error)
                }
            })?
            .get_json("/{id}/history", move |request| {
                let service = Arc::clone(&history_service);
                async move {
                    let id = required_param(&request, "id")?;
                    service.run_history(&id).await.map_err(map_error)
                }
            })?
            .get_json("/{id}/node-executions", move |request| {
                let service = Arc::clone(&execution_service);
                async move {
                    let id = required_param(&request, "id")?;
                    service.node_executions(&id).await.map_err(map_error)
                }
            })?
            .post("/{id}/cancel", move |request: BootRequest| {
                let service = Arc::clone(&cancel_service);
                async move {
                    let id = required_param(&request, "id")?;
                    let payload = request.json::<CancelRunRequest>()?;
                    service
                        .cancel_run(&id, payload.reason)
                        .await
                        .map_err(map_error)?;
                    Ok(BootResponse::no_content())
                }
            })?
            .post("/{id}/approvals/{nodeId}", move |request: BootRequest| {
                let service = Arc::clone(&approval_service);
                async move {
                    let id = required_param(&request, "id")?;
                    let node_id = required_param(&request, "nodeId")?;
                    let payload = request.json::<ApprovalRequest>()?;
                    let run = service
                        .resume_approval(&id, &node_id, payload.payload)
                        .await
                        .map_err(map_error)?;
                    BootResponse::json(&run)
                }
            })
    }

    fn catalog_controller() -> Result<ControllerDefinition> {
        ControllerDefinition::new("/api/v1/node-types")?.get_json("/", |_| async {
            Ok(vec![
                NodeTypeDescriptor {
                    kind: "start",
                    label: "Input",
                    description: "Supplies the JSON payload that starts a run.",
                    default_config: json!({}),
                },
                NodeTypeDescriptor {
                    kind: "template",
                    label: "Template",
                    description: "Builds JSON using input.* and steps.<node>.* tokens.",
                    default_config: json!({ "value": { "message": "Hello, {{input.name}}!" } }),
                },
                NodeTypeDescriptor {
                    kind: "llm",
                    label: "LLM",
                    description: "Calls an OpenAI-compatible model through A3S Gateway.",
                    default_config: json!({ "model": "", "system": "", "prompt": "{{input.prompt}}" }),
                },
                NodeTypeDescriptor {
                    kind: "agent",
                    label: "Agent",
                    description: "Runs a bounded model/tool loop as an isolated Runtime task.",
                    default_config: json!({ "model": "", "prompt": "{{input.prompt}}", "maxIterations": 6, "tools": [] }),
                },
                NodeTypeDescriptor {
                    kind: "tool",
                    label: "Tool",
                    description: "Invokes an allow-listed tool endpoint with secret references.",
                    default_config: json!({ "method": "POST", "endpoint": "https://api.example.com", "body": {} }),
                },
                NodeTypeDescriptor {
                    kind: "router",
                    label: "Router",
                    description: "Selects one named graph route from typed conditions.",
                    default_config: json!({ "routes": [], "default": "default" }),
                },
                NodeTypeDescriptor {
                    kind: "memory",
                    label: "Memory",
                    description: "Stores or retrieves Agent memory through the PostgreSQL-backed A3S Memory boundary.",
                    default_config: json!({ "operation": "search", "query": "{{input.query}}", "limit": 5 }),
                },
                NodeTypeDescriptor {
                    kind: "http",
                    label: "HTTP",
                    description: "Calls an allow-listed HTTP endpoint as a durable step.",
                    default_config: json!({ "method": "GET", "url": "https://api.example.com" }),
                },
                NodeTypeDescriptor {
                    kind: "approval",
                    label: "Approval",
                    description: "Suspends the durable run until a human responds.",
                    default_config: json!({ "message": "Approve this run?" }),
                },
                NodeTypeDescriptor {
                    kind: "output",
                    label: "Output",
                    description: "Completes the run with its upstream value.",
                    default_config: json!({}),
                },
            ])
        })
    }

    fn internal_controller(service: Arc<WorkflowService>) -> Result<ControllerDefinition> {
        ControllerDefinition::new("/internal/v1/node-executions")?.get(
            "/{id}/invocation",
            move |request: BootRequest| {
                let service = Arc::clone(&service);
                async move {
                    let id = required_param(&request, "id")?;
                    let token = request.query_param("token").ok_or_else(|| {
                        BootError::Unauthorized("missing artifact token".to_string())
                    })?;
                    let invocation = service
                        .node_invocation(&id, token)
                        .await
                        .map_err(map_error)?;
                    let bytes = serde_json::to_vec(&invocation)
                        .map_err(|error| BootError::Internal(error.to_string()))?;
                    Ok(BootResponse::new(200, bytes)
                        .with_header("content-type", NODE_INVOCATION_MEDIA_TYPE)
                        .with_header("cache-control", "private, immutable"))
                }
            },
        )
    }
}

impl Module for WorkflowModule {
    fn name(&self) -> &'static str {
        "workflow"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::from_arc(Arc::clone(
            &self.service,
        ))])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let service = module_ref.get::<WorkflowService>()?;
        Ok(vec![
            Self::workflow_controller(Arc::clone(&service))?,
            Self::run_controller(Arc::clone(&service))?,
            Self::catalog_controller()?,
            Self::internal_controller(service)?,
        ])
    }
}

fn required_param(request: &BootRequest, name: &str) -> Result<String> {
    request
        .param(name)
        .map(str::to_string)
        .ok_or_else(|| BootError::BadRequest(format!("missing path parameter {name}")))
}

fn map_error(error: WorkflowError) -> BootError {
    match error {
        WorkflowError::Validation(message) => BootError::UnprocessableEntity(message),
        WorkflowError::NotFound(id) => BootError::NotFound(format!("workflow or run {id}")),
        WorkflowError::Conflict(message) => BootError::Conflict(message),
        WorkflowError::External(message) => BootError::BadGateway(message),
        WorkflowError::Persistence(message) => BootError::Internal(message),
        WorkflowError::Execution(message) => BootError::Internal(message),
    }
}
