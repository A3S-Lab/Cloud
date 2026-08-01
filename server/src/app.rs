use std::sync::Arc;

use a3s_boot::{BootApplication, ConfigModule, CorsOptions, HttpMethod, SecurityHeadersOptions};
use a3s_event::{EventBus, MemoryProvider};
use a3s_flow::{
    A3sEventBusFlowEventSink, A3sFlowEventBridge, FanoutFlowEventObserver, FlowEngine,
    LocalFileA3sFlowEventSink, PostgresEventStore, PostgresFlowTaskQueue,
};
use sqlx::postgres::PgPoolOptions;

use crate::config::AppConfig;
use crate::modules::health;
use crate::modules::workflow::application::WorkflowService;
use crate::modules::workflow::infrastructure::{
    GraphRuntime, GraphRuntimeConfig, PostgresMemoryStore, PostgresNodeExecutionStore,
    PostgresWorkflowRepository,
};
use crate::modules::workflow::presentation::{MemoryModule, WorkflowModule};

pub struct ApplicationServices {
    pub application: BootApplication,
    pub engine: FlowEngine,
    pub queue: Arc<PostgresFlowTaskQueue>,
}

pub async fn build_application(config: AppConfig) -> anyhow::Result<ApplicationServices> {
    // API and worker replicas may start at the same time. PostgreSQL's
    // `CREATE TABLE IF NOT EXISTS` can still race at the system-catalog level,
    // so serialize the complete schema/bootstrap phase across every replica.
    let migration_lock_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&config.storage.database_url)
        .await?;
    let mut migration_lock = migration_lock_pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind("a3s-workflow-schema-v1")
        .execute(&mut *migration_lock)
        .await?;

    let repository = PostgresWorkflowRepository::connect(
        &config.storage.database_url,
        config.storage.max_connections,
    )
    .await?;
    let executions = Arc::new(
        PostgresNodeExecutionStore::connect(
            &config.storage.database_url,
            config.storage.max_connections,
        )
        .await?,
    );
    let memory_store = Arc::new(
        PostgresMemoryStore::connect(&config.storage.database_url, config.storage.max_connections)
            .await?,
    );
    let runtime = Arc::new(GraphRuntime::new(
        GraphRuntimeConfig {
            runtime: config.runtime.clone(),
            providers: config.runtimes.clone(),
            gateway: config.gateway.clone(),
            memory: config.memory.clone(),
            http_allowed_hosts: config
                .security
                .http_allowed_hosts
                .iter()
                .map(|host| host.to_ascii_lowercase())
                .collect(),
            max_http_response_bytes: config.security.max_http_response_bytes,
        },
        Arc::clone(&executions),
    )?);

    let event_bus = Arc::new(EventBus::new(MemoryProvider::default()));
    let event_sink = Arc::new(A3sEventBusFlowEventSink::new(Arc::clone(&event_bus)));
    let event_bridge = Arc::new(A3sFlowEventBridge::new(event_sink));
    let audit_sink = Arc::new(LocalFileA3sFlowEventSink::new(&config.storage.audit_path));
    let audit_bridge = Arc::new(A3sFlowEventBridge::new(audit_sink));
    let observer = Arc::new(
        FanoutFlowEventObserver::new()
            .with_observer(event_bridge)
            .with_observer(audit_bridge),
    );
    let flow_store = Arc::new(PostgresEventStore::connect(&config.storage.database_url).await?);
    let engine = FlowEngine::builder(runtime)
        .with_store(flow_store)
        .with_observer(observer)
        .build();
    let queue = Arc::new(
        PostgresFlowTaskQueue::connect_with_queue(
            &config.storage.database_url,
            &config.flow.queue_name,
        )
        .await?,
    );
    let workflow_service = Arc::new(WorkflowService::new(
        Arc::new(repository.clone()),
        engine.clone(),
        Arc::clone(&queue) as Arc<dyn a3s_flow::FlowTaskQueue>,
        executions,
        Arc::clone(&event_bus),
    ));
    if config.storage.seed_sample {
        workflow_service.ensure_sample().await?;
    }
    migration_lock.commit().await?;

    let cors = config.server.cors_origins.iter().fold(
        CorsOptions::new()
            .allow_methods([
                HttpMethod::Get,
                HttpMethod::Post,
                HttpMethod::Put,
                HttpMethod::Delete,
                HttpMethod::Options,
            ])
            .allow_headers(["content-type", "authorization"])
            .with_max_age(600),
        |cors, origin| cors.allow_origin(origin),
    );
    let application = BootApplication::builder()
        .use_global_cors(cors)
        .use_global_security_headers(SecurityHeadersOptions::new())
        .import(ConfigModule::from_value("workflow-config", config).global())
        .import(health::module(repository))
        .import(MemoryModule::new(
            memory_store as Arc<dyn a3s_memory::MemoryStore>,
        ))
        .import(WorkflowModule::new(Arc::clone(&workflow_service)))
        .build()?;

    Ok(ApplicationServices {
        application,
        engine,
        queue,
    })
}

#[cfg(test)]
mod tests {
    use a3s_boot::{BootRequest, HttpMethod};
    use a3s_flow::WorkflowRunSnapshot;
    use serde_json::{json, Value};
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::*;
    use crate::modules::workflow::application::StartRunRequest;
    use crate::modules::workflow::domain::{
        NodeData, NodeKind, NodeRuntimePolicy, Position, WorkflowDraft, WorkflowEdge, WorkflowNode,
        WorkflowUpdate,
    };

    fn test_database_url() -> Option<String> {
        std::env::var("A3S_WORKFLOW_TEST_DATABASE_URL").ok()
    }

    fn test_config(database_url: String, temporary: &TempDir) -> AppConfig {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/workflow.acl");
        let mut config = AppConfig::from_acl_file(path).expect("load application config");
        config.storage.database_url = database_url;
        config.storage.max_connections = 4;
        config.storage.audit_path = temporary
            .path()
            .join("flow-events.jsonl")
            .to_string_lossy()
            .into_owned();
        config.storage.seed_sample = false;
        config.flow.queue_name = format!("coverage-{}", Uuid::new_v4());
        config
    }

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

    fn draft(name: &str) -> WorkflowDraft {
        WorkflowDraft {
            name: name.to_string(),
            description: "API contract coverage".to_string(),
            nodes: vec![
                node("start", NodeKind::Start),
                node("output", NodeKind::Output),
            ],
            edges: vec![WorkflowEdge {
                id: "start-output".to_string(),
                source: "start".to_string(),
                target: "output".to_string(),
                source_handle: None,
            }],
        }
    }

    fn json_request<T: serde::Serialize>(
        method: HttpMethod,
        path: impl Into<String>,
        body: &T,
    ) -> BootRequest {
        BootRequest::new(method, path)
            .with_json(body)
            .expect("serialize request body")
    }

    #[tokio::test]
    async fn application_routes_cover_durable_workflow_and_memory_contracts() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let temporary = TempDir::new().expect("temporary audit directory");
        let services = build_application(test_config(database_url, &temporary))
            .await
            .expect("build application");
        let app = services.application;

        let health = app
            .handle(BootRequest::new(HttpMethod::Get, "/api/health"))
            .await;
        assert_eq!(health.status(), 200);
        assert_eq!(
            health.body_json::<Value>().expect("health JSON")["checks"]["postgres"]["status"],
            "up"
        );

        let catalog = app
            .handle(BootRequest::new(HttpMethod::Get, "/api/v1/node-types"))
            .await;
        assert_eq!(catalog.status(), 200);
        assert_eq!(
            catalog
                .body_json::<Vec<Value>>()
                .expect("node catalog")
                .len(),
            10
        );

        let invalid = app
            .handle(json_request(
                HttpMethod::Post,
                "/api/v1/workflows",
                &json!({"name": "", "nodes": [], "edges": []}),
            ))
            .await;
        assert_eq!(invalid.status(), 422);

        let created = app
            .handle(json_request(
                HttpMethod::Post,
                "/api/v1/workflows",
                &draft("  Coverage workflow  "),
            ))
            .await;
        assert_eq!(created.status(), 201);
        let workflow = created
            .body_json::<crate::modules::workflow::domain::WorkflowDefinition>()
            .expect("created workflow");
        assert_eq!(workflow.name, "Coverage workflow");

        let listed = app
            .handle(BootRequest::new(HttpMethod::Get, "/api/v1/workflows"))
            .await;
        assert!(listed
            .body_json::<Vec<Value>>()
            .expect("workflow list")
            .iter()
            .any(|value| value["id"] == workflow.id));

        let fetched = app
            .handle(BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/workflows/{}", workflow.id),
            ))
            .await;
        assert_eq!(fetched.status(), 200);

        let update = WorkflowUpdate {
            version: workflow.version,
            name: "Updated coverage workflow".to_string(),
            description: "updated".to_string(),
            nodes: workflow.nodes.clone(),
            edges: workflow.edges.clone(),
        };
        let updated = app
            .handle(json_request(
                HttpMethod::Put,
                format!("/api/v1/workflows/{}", workflow.id),
                &update,
            ))
            .await;
        assert_eq!(updated.status(), 200);
        assert_eq!(
            updated.body_json::<Value>().expect("updated workflow")["version"],
            2
        );

        let stale = app
            .handle(json_request(
                HttpMethod::Put,
                format!("/api/v1/workflows/{}", workflow.id),
                &update,
            ))
            .await;
        assert_eq!(stale.status(), 409);

        let started = app
            .handle(json_request(
                HttpMethod::Post,
                format!("/api/v1/workflows/{}/runs", workflow.id),
                &StartRunRequest {
                    input: json!({"name": "coverage"}),
                },
            ))
            .await;
        assert_eq!(started.status(), 201);
        let run = started
            .body_json::<WorkflowRunSnapshot>()
            .expect("started run");

        let all_runs = app
            .handle(BootRequest::new(HttpMethod::Get, "/api/v1/runs"))
            .await;
        assert!(all_runs
            .body_json::<Vec<Value>>()
            .expect("run list")
            .iter()
            .any(|value| value["run_id"] == run.run_id));

        let filtered_runs = app
            .handle(BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/runs?workflowId={}", workflow.id),
            ))
            .await;
        assert_eq!(
            filtered_runs
                .body_json::<Vec<Value>>()
                .expect("filtered runs")
                .len(),
            1
        );

        for path in [
            format!("/api/v1/runs/{}", run.run_id),
            format!("/api/v1/runs/{}/history", run.run_id),
            format!("/api/v1/runs/{}/node-executions", run.run_id),
            "/api/v1/runs/summary".to_string(),
            "/api/v1/runs/events?limit=5000".to_string(),
        ] {
            let response = app.handle(BootRequest::new(HttpMethod::Get, path)).await;
            assert_eq!(response.status(), 200);
        }

        let missing_approval = app
            .handle(json_request(
                HttpMethod::Post,
                format!("/api/v1/runs/{}/approvals/review", run.run_id),
                &json!({"payload": {"approved": true}}),
            ))
            .await;
        assert_eq!(missing_approval.status(), 404);

        let cancelled = app
            .handle(json_request(
                HttpMethod::Post,
                format!("/api/v1/runs/{}/cancel", run.run_id),
                &json!({"reason": "coverage cleanup"}),
            ))
            .await;
        assert_eq!(cancelled.status(), 204);

        let missing_token = app
            .handle(BootRequest::new(
                HttpMethod::Get,
                "/internal/v1/node-executions/missing/invocation",
            ))
            .await;
        assert_eq!(missing_token.status(), 401);
        let missing_invocation = app
            .handle(BootRequest::new(
                HttpMethod::Get,
                "/internal/v1/node-executions/missing/invocation?token=invalid",
            ))
            .await;
        assert_eq!(missing_invocation.status(), 404);

        let stored = app
            .handle(json_request(
                HttpMethod::Post,
                "/api/v1/memories:store",
                &json!({
                    "content": "Runtime evidence belongs in PostgreSQL",
                    "importance": 0.9,
                    "tags": ["runtime", "coverage"],
                    "memoryType": "semantic",
                    "metadata": {"source": "contract-test"}
                }),
            ))
            .await;
        assert_eq!(stored.status(), 201);
        let memory = stored.body_json::<Value>().expect("stored memory");
        let memory_id = memory["id"].as_str().expect("memory id");

        let retrieved = app
            .handle(BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/memories/{memory_id}"),
            ))
            .await;
        assert_eq!(retrieved.status(), 200);

        for body in [
            json!({"query": "PostgreSQL", "limit": 5}),
            json!({"tags": ["runtime"], "limit": 5}),
        ] {
            let searched = app
                .handle(json_request(
                    HttpMethod::Post,
                    "/api/v1/memories:search",
                    &body,
                ))
                .await;
            assert_eq!(searched.status(), 200);
            assert!(!searched
                .body_json::<Vec<Value>>()
                .expect("memory search")
                .is_empty());
        }

        let blank_memory = app
            .handle(json_request(
                HttpMethod::Post,
                "/api/v1/memories:store",
                &json!({"content": "   "}),
            ))
            .await;
        assert_eq!(blank_memory.status(), 422);

        let deleted_memory = app
            .handle(BootRequest::new(
                HttpMethod::Delete,
                format!("/api/v1/memories/{memory_id}"),
            ))
            .await;
        assert_eq!(deleted_memory.status(), 204);
        let missing_memory = app
            .handle(BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/memories/{memory_id}"),
            ))
            .await;
        assert_eq!(missing_memory.status(), 404);

        let service = app
            .get::<WorkflowService>()
            .expect("workflow service provider");
        let _ = app
            .handle(BootRequest::new(
                HttpMethod::Delete,
                "/api/v1/workflows/welcome-workflow",
            ))
            .await;
        service.ensure_sample().await.expect("seed sample");
        service.ensure_sample().await.expect("sample is idempotent");

        for id in [&workflow.id, "welcome-workflow"] {
            let deleted = app
                .handle(BootRequest::new(
                    HttpMethod::Delete,
                    format!("/api/v1/workflows/{id}"),
                ))
                .await;
            assert_eq!(deleted.status(), 204);
            let missing = app
                .handle(BootRequest::new(
                    HttpMethod::Get,
                    format!("/api/v1/workflows/{id}"),
                ))
                .await;
            assert_eq!(missing.status(), 404);
        }
    }
}
