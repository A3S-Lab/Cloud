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
