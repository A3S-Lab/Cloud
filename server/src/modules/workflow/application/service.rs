use std::sync::Arc;

use a3s_event::{Event, EventBus};
use a3s_flow::{
    FlowEngine, FlowEvent, FlowEventEnvelope, FlowTask, FlowTaskQueue, HookStatus,
    WorkflowRunSnapshot, WorkflowRunSummary, WorkflowSpec,
};
use a3s_workflow_protocol::NodeInvocation;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::modules::workflow::domain::{
    NodeData, NodeKind, NodeRuntimePolicy, Position, WorkflowDefinition, WorkflowDraft,
    WorkflowEdge, WorkflowError, WorkflowNode, WorkflowRepository, WorkflowResult, WorkflowUpdate,
};
use crate::modules::workflow::infrastructure::{NodeExecutionEvidence, PostgresNodeExecutionStore};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunRequest {
    #[serde(default)]
    pub input: Value,
}

#[derive(Clone)]
pub struct WorkflowService {
    repository: Arc<dyn WorkflowRepository>,
    engine: FlowEngine,
    queue: Arc<dyn FlowTaskQueue>,
    executions: Arc<PostgresNodeExecutionStore>,
    event_bus: Arc<EventBus>,
}

impl WorkflowService {
    pub fn new(
        repository: Arc<dyn WorkflowRepository>,
        engine: FlowEngine,
        queue: Arc<dyn FlowTaskQueue>,
        executions: Arc<PostgresNodeExecutionStore>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            repository,
            engine,
            queue,
            executions,
            event_bus,
        }
    }

    pub async fn list(&self) -> WorkflowResult<Vec<WorkflowDefinition>> {
        self.repository.list().await
    }

    pub async fn get(&self, id: &str) -> WorkflowResult<WorkflowDefinition> {
        self.repository
            .find(id)
            .await?
            .ok_or_else(|| WorkflowError::NotFound(id.to_string()))
    }

    pub async fn create(&self, draft: WorkflowDraft) -> WorkflowResult<WorkflowDefinition> {
        draft.validate()?;
        let now = Utc::now();
        let workflow = WorkflowDefinition {
            id: Uuid::new_v4().to_string(),
            name: draft.name.trim().to_string(),
            description: draft.description.trim().to_string(),
            version: 1,
            nodes: draft.nodes,
            edges: draft.edges,
            created_at: now,
            updated_at: now,
        };
        workflow.validate()?;
        self.repository.create(&workflow).await?;
        Ok(workflow)
    }

    pub async fn update(
        &self,
        id: &str,
        update: WorkflowUpdate,
    ) -> WorkflowResult<WorkflowDefinition> {
        update.validate()?;
        let current = self.get(id).await?;
        if current.version != update.version {
            return Err(WorkflowError::Conflict(format!(
                "expected version {}, current version is {}",
                update.version, current.version
            )));
        }
        let workflow = WorkflowDefinition {
            id: current.id,
            name: update.name.trim().to_string(),
            description: update.description.trim().to_string(),
            version: current.version + 1,
            nodes: update.nodes,
            edges: update.edges,
            created_at: current.created_at,
            updated_at: Utc::now(),
        };
        workflow.validate()?;
        self.repository.update(&workflow, current.version).await?;
        Ok(workflow)
    }

    pub async fn delete(&self, id: &str) -> WorkflowResult<()> {
        if !self.repository.delete(id).await? {
            return Err(WorkflowError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub async fn start_run(
        &self,
        workflow_id: &str,
        request: StartRunRequest,
    ) -> WorkflowResult<WorkflowRunSnapshot> {
        let definition = self.get(workflow_id).await?;
        let spec = WorkflowSpec::rust_embedded(
            format!("workflow.{}", definition.id),
            definition.version.to_string(),
            "a3s_workflow::graph_runtime",
            "run",
        );
        spec.validate().map_err(execution_error)?;
        let run_id = Uuid::new_v4().to_string();
        let input = json!({
            "definition": definition,
            "input": request.input,
        });
        let created = self
            .engine
            .store()
            .append_if_sequence(&run_id, 0, FlowEvent::RunCreated { spec, input })
            .await
            .map_err(execution_error)?;
        self.engine.observer().observe(created.clone()).await;
        let started = self
            .engine
            .store()
            .append_if_sequence(&run_id, created.sequence, FlowEvent::RunStarted)
            .await
            .map_err(execution_error)?;
        self.engine.observer().observe(started).await;
        self.queue
            .enqueue(FlowTask::DriveRun {
                run_id: run_id.clone(),
            })
            .await
            .map_err(execution_error)?;
        self.engine.snapshot(&run_id).await.map_err(execution_error)
    }

    pub async fn list_runs(
        &self,
        workflow_id: Option<&str>,
    ) -> WorkflowResult<Vec<WorkflowRunSnapshot>> {
        let mut snapshots = self
            .engine
            .list_snapshots()
            .await
            .map_err(execution_error)?;
        if let Some(workflow_id) = workflow_id {
            let expected = format!("workflow.{workflow_id}");
            snapshots.retain(|snapshot| snapshot.spec.name == expected);
        }
        Ok(snapshots)
    }

    pub async fn get_run(&self, run_id: &str) -> WorkflowResult<WorkflowRunSnapshot> {
        self.engine.snapshot(run_id).await.map_err(execution_error)
    }

    pub async fn run_history(&self, run_id: &str) -> WorkflowResult<Vec<FlowEventEnvelope>> {
        self.engine.history(run_id).await.map_err(execution_error)
    }

    pub async fn node_executions(
        &self,
        run_id: &str,
    ) -> WorkflowResult<Vec<NodeExecutionEvidence>> {
        self.engine
            .snapshot(run_id)
            .await
            .map_err(execution_error)?;
        self.executions.list_for_run(run_id).await
    }

    pub async fn node_invocation(
        &self,
        execution_id: &str,
        token: &str,
    ) -> WorkflowResult<NodeInvocation> {
        self.executions
            .invocation(execution_id, token)
            .await?
            .ok_or_else(|| WorkflowError::NotFound(execution_id.to_string()))
    }

    pub async fn run_summary(&self) -> WorkflowResult<WorkflowRunSummary> {
        self.engine.run_summary().await.map_err(execution_error)
    }

    pub async fn cancel_run(&self, run_id: &str, reason: Option<String>) -> WorkflowResult<()> {
        self.engine
            .cancel(run_id, reason)
            .await
            .map_err(execution_error)
    }

    pub async fn resume_approval(
        &self,
        run_id: &str,
        node_id: &str,
        payload: Value,
    ) -> WorkflowResult<WorkflowRunSnapshot> {
        let snapshot = self
            .engine
            .snapshot(run_id)
            .await
            .map_err(execution_error)?;
        let hook = snapshot.hooks.get(node_id).ok_or_else(|| {
            WorkflowError::NotFound(format!("approval {node_id} for run {run_id}"))
        })?;
        if hook.status != HookStatus::Active {
            return Err(WorkflowError::Conflict(format!(
                "approval {node_id} is not active"
            )));
        }
        self.queue
            .enqueue(FlowTask::ResumeHook {
                run_id: run_id.to_string(),
                hook_id: node_id.to_string(),
                payload,
            })
            .await
            .map_err(execution_error)?;
        self.engine.snapshot(run_id).await.map_err(execution_error)
    }

    pub async fn recent_events(&self, limit: usize) -> WorkflowResult<Vec<Event>> {
        self.event_bus
            .list_events(Some("flow"), limit.min(500))
            .await
            .map_err(|error| WorkflowError::Execution(error.to_string()))
    }

    pub async fn ensure_sample(&self) -> WorkflowResult<()> {
        const SAMPLE_ID: &str = "welcome-workflow";
        if self.repository.find(SAMPLE_ID).await?.is_some() {
            return Ok(());
        }
        let now = Utc::now();
        let workflow = WorkflowDefinition {
            id: SAMPLE_ID.to_string(),
            name: "欢迎使用 A3S Workflow".to_string(),
            description: "一个由 A3S Flow 持久化执行的工作流。".to_string(),
            version: 1,
            nodes: vec![
                WorkflowNode {
                    id: "start".to_string(),
                    kind: NodeKind::Start,
                    position: Position { x: 40.0, y: 180.0 },
                    data: NodeData {
                        label: "输入".to_string(),
                        config: json!({}),
                        runtime: NodeRuntimePolicy::default(),
                    },
                },
                WorkflowNode {
                    id: "greeting".to_string(),
                    kind: NodeKind::Template,
                    position: Position { x: 340.0, y: 180.0 },
                    data: NodeData {
                        label: "生成问候语".to_string(),
                        config: json!({
                            "value": {
                                "message": "你好，{{input.name}}！",
                                "engine": "a3s-flow"
                            }
                        }),
                        runtime: NodeRuntimePolicy::default(),
                    },
                },
                WorkflowNode {
                    id: "output".to_string(),
                    kind: NodeKind::Output,
                    position: Position { x: 640.0, y: 180.0 },
                    data: NodeData {
                        label: "结果".to_string(),
                        config: json!({}),
                        runtime: NodeRuntimePolicy::default(),
                    },
                },
            ],
            edges: vec![
                WorkflowEdge {
                    id: "start-greeting".to_string(),
                    source: "start".to_string(),
                    target: "greeting".to_string(),
                    source_handle: None,
                },
                WorkflowEdge {
                    id: "greeting-output".to_string(),
                    source: "greeting".to_string(),
                    target: "output".to_string(),
                    source_handle: None,
                },
            ],
            created_at: now,
            updated_at: now,
        };
        workflow.validate()?;
        self.repository.create(&workflow).await
    }
}

fn execution_error(error: a3s_flow::FlowError) -> WorkflowError {
    WorkflowError::Execution(error.to_string())
}
