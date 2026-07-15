use a3s_boot::HealthIndicatorResult;
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowScheduler, FlowTaskQueue, FlowWorker,
    PostgresEventStore, PostgresFlowTaskQueue,
};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
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

#[derive(Debug, thiserror::Error)]
pub enum FlowCoordinatorError {
    #[error("operation reconciliation failed: {0}")]
    Repository(#[from] crate::modules::shared_kernel::domain::RepositoryError),
    #[error("A3S Flow coordination failed: {0}")]
    Flow(#[from] FlowError),
    #[error("operation Flow lease duration exceeds the supported range")]
    InvalidLease,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowCoordinatorReport {
    pub reconciled_before_work: usize,
    pub reconciled_after_work: usize,
    pub reconciliation_failures: usize,
    pub recovered_tasks: usize,
    pub enqueued_tasks: usize,
    pub handled_tasks: usize,
}

#[derive(Clone)]
pub struct FlowInfrastructure {
    engine: FlowEngine,
    queue: Arc<PostgresFlowTaskQueue>,
}

pub struct FlowOperationCoordinator {
    reconciler: crate::modules::operations::OperationReconciler,
    scheduler: FlowScheduler,
    worker: FlowWorker,
    queue: Arc<PostgresFlowTaskQueue>,
    interval: Duration,
    lease_duration: chrono::Duration,
}

impl FlowOperationCoordinator {
    pub fn new(
        reconciler: crate::modules::operations::OperationReconciler,
        flow: &FlowInfrastructure,
        interval: Duration,
        lease_duration: Duration,
    ) -> Result<Self, FlowCoordinatorError> {
        let lease_duration = chrono::Duration::from_std(lease_duration)
            .map_err(|_| FlowCoordinatorError::InvalidLease)?;
        Ok(Self {
            reconciler,
            scheduler: FlowScheduler::new(flow.engine(), flow.queue()),
            worker: FlowWorker::new(flow.engine(), flow.queue()),
            queue: flow.queue(),
            interval,
            lease_duration,
        })
    }

    pub async fn run_once(&self) -> Result<FlowCoordinatorReport, FlowCoordinatorError> {
        let recovered_tasks = self
            .queue
            .requeue_inflight_older_than(Utc::now() - self.lease_duration)
            .await?;
        let before = self.reconciler.run_once().await?;
        let tick = self.scheduler.enqueue_due_work(Utc::now()).await?;
        let handled_tasks = self.worker.run_until_idle().await?.len();
        let after = self.reconciler.run_once().await?;
        Ok(FlowCoordinatorReport {
            reconciled_before_work: before.projected,
            reconciled_after_work: after.projected,
            reconciliation_failures: before.failures.len() + after.failures.len(),
            recovered_tasks,
            enqueued_tasks: tick.enqueued_tasks,
            handled_tasks,
        })
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once().await {
                        Ok(report) => {
                            if report.reconciliation_failures > 0 {
                                tracing::warn!(
                                    failures = report.reconciliation_failures,
                                    "operation Flow cycle completed with reconciliation failures"
                                );
                            }
                            tracing::debug!(
                                recovered_tasks = report.recovered_tasks,
                                enqueued_tasks = report.enqueued_tasks,
                                handled_tasks = report.handled_tasks,
                                projected = report.reconciled_before_work + report.reconciled_after_work,
                                "operation Flow cycle completed"
                            );
                        }
                        Err(error) => tracing::error!(error = %error, "operation Flow cycle failed"),
                    }
                }
            }
        }
    }
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
    runtime: Arc<dyn FlowRuntime>,
) -> Result<FlowInfrastructure, FlowInfrastructureError> {
    FlowInfrastructure::connect(database_url, runtime).await
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
