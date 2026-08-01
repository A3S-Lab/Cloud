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
                    label: "开始",
                    description: "定义工作流的初始输入参数。",
                    default_config: json!({}),
                },
                NodeTypeDescriptor {
                    kind: "template",
                    label: "模板转换",
                    description: "使用 input.* 和 steps.<node>.* 变量构建 JSON。",
                    default_config: json!({ "value": { "message": "你好，{{input.name}}！" } }),
                },
                NodeTypeDescriptor {
                    kind: "llm",
                    label: "大语言模型",
                    description: "通过 A3S Gateway 调用 OpenAI 兼容模型。",
                    default_config: json!({ "model": "", "system": "", "prompt": "{{input.prompt}}" }),
                },
                NodeTypeDescriptor {
                    kind: "agent",
                    label: "智能体",
                    description: "以隔离的 Runtime 任务运行有界模型与工具循环。",
                    default_config: json!({ "model": "", "prompt": "{{input.prompt}}", "maxIterations": 6, "tools": [] }),
                },
                NodeTypeDescriptor {
                    kind: "tool",
                    label: "工具",
                    description: "使用密钥引用调用白名单中的工具端点。",
                    default_config: json!({ "method": "POST", "endpoint": "https://api.example.com", "body": {} }),
                },
                NodeTypeDescriptor {
                    kind: "router",
                    label: "条件分支",
                    description: "根据类型化条件选择一个具名执行分支。",
                    default_config: json!({ "routes": [], "default": "default" }),
                },
                NodeTypeDescriptor {
                    kind: "memory",
                    label: "记忆",
                    description: "通过 PostgreSQL 支持的 A3S Memory 边界存取智能体记忆。",
                    default_config: json!({ "operation": "search", "query": "{{input.query}}", "limit": 5 }),
                },
                NodeTypeDescriptor {
                    kind: "http",
                    label: "HTTP 请求",
                    description: "以持久化步骤调用白名单中的 HTTP 端点。",
                    default_config: json!({ "method": "GET", "url": "https://api.example.com" }),
                },
                NodeTypeDescriptor {
                    kind: "approval",
                    label: "人工审批",
                    description: "暂停持久化运行，等待人工响应。",
                    default_config: json!({ "message": "是否批准本次运行？" }),
                },
                NodeTypeDescriptor {
                    kind: "output",
                    label: "结束",
                    description: "使用上游结果完成本次运行。",
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
