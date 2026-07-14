use a3s_boot::HealthIndicatorResult;
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowTaskQueue, PostgresEventStore, PostgresFlowTaskQueue,
    RuntimeCommand, StepInvocation, WorkflowInvocation,
};
use async_trait::async_trait;
use std::sync::Arc;
use url::Url;

const FLOW_SCHEMA: &str = "a3s_flow";
const FLOW_QUEUE: &str = "cloud-operations";

#[derive(Debug, thiserror::Error)]
pub enum FlowInfrastructureError {
    #[error("invalid PostgreSQL URL for A3S Flow: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("the PostgreSQL URL cannot define options because Cloud owns the Flow search path")]
    ConflictingOptions,
    #[error("could not initialize A3S Flow: {0}")]
    Flow(#[from] FlowError),
}

#[derive(Clone)]
pub struct FlowInfrastructure {
    engine: FlowEngine,
    queue: Arc<PostgresFlowTaskQueue>,
}

impl FlowInfrastructure {
    pub async fn connect(
        database_url: &str,
        runtime: Arc<dyn FlowRuntime>,
    ) -> Result<Self, FlowInfrastructureError> {
        let flow_url = scoped_postgres_url(database_url)?;
        let store = Arc::new(PostgresEventStore::connect(flow_url.as_str()).await?);
        let queue = Arc::new(
            PostgresFlowTaskQueue::connect_with_queue(flow_url.as_str(), FLOW_QUEUE).await?,
        );
        Ok(Self {
            engine: FlowEngine::new(store, runtime),
            queue,
        })
    }

    pub fn engine(&self) -> FlowEngine {
        self.engine.clone()
    }

    pub fn queue(&self) -> Arc<PostgresFlowTaskQueue> {
        Arc::clone(&self.queue)
    }

    pub async fn health(&self) -> HealthIndicatorResult {
        let runs = match self.engine.list_run_ids().await {
            Ok(runs) => runs.len(),
            Err(error) => {
                return HealthIndicatorResult::down().with_detail_value("error", error.to_string())
            }
        };
        match self.queue.len().await {
            Ok(pending) => HealthIndicatorResult::up()
                .with_detail_value("runs", runs)
                .with_detail_value("pendingTasks", pending),
            Err(error) => {
                HealthIndicatorResult::down().with_detail_value("error", error.to_string())
            }
        }
    }
}

pub async fn connect_flow(
    database_url: &str,
) -> Result<FlowInfrastructure, FlowInfrastructureError> {
    FlowInfrastructure::connect(database_url, Arc::new(DeferredCloudRuntime)).await
}

fn scoped_postgres_url(database_url: &str) -> Result<Url, FlowInfrastructureError> {
    let mut url = Url::parse(database_url)?;
    if url.query_pairs().any(|(key, _)| key == "options") {
        return Err(FlowInfrastructureError::ConflictingOptions);
    }
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={FLOW_SCHEMA}"));
    Ok(url)
}

#[derive(Debug, Clone, Copy)]
struct DeferredCloudRuntime;

#[async_trait]
impl FlowRuntime for DeferredCloudRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Err(FlowError::Runtime(format!(
            "Cloud workflow {:?} has no registered runtime",
            invocation.spec.name
        )))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(format!(
            "Cloud step {:?} has no registered runtime",
            invocation.step_name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_url_owns_an_isolated_search_path() -> Result<(), FlowInfrastructureError> {
        let url =
            scoped_postgres_url("postgres://user:secret@localhost/cloud?application_name=a3s")?;
        let query = url.query().unwrap_or_default();
        assert!(query.contains("application_name=a3s"));
        assert!(query.contains("options=-csearch_path%3Da3s_flow"));
        assert!(matches!(
            scoped_postgres_url("postgres://localhost/cloud?options=-cfoo%3Dbar"),
            Err(FlowInfrastructureError::ConflictingOptions)
        ));
        Ok(())
    }
}
