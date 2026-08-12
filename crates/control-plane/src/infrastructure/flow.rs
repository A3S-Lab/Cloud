use a3s_boot::{
    BootError, HealthIndicatorResult, ModuleRef, PostgresQueueBackend, Queue, QueueOptions,
    QueueRetryPolicy, QueueStats,
};
use a3s_flow::{
    BootFlowTaskDeduplication, BootFlowTaskManager, BootFlowTaskPolicy, FlowEngine, FlowError,
    FlowRuntime, FlowScheduler, PostgresEventStore, RuntimeBuildCompatibility, RuntimeBuildId,
    RuntimeCommand, StepInvocation, WorkflowInvocation,
};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use url::Url;

const FLOW_SCHEMA: &str = "a3s_flow";
const BOOT_SCHEMA: &str = "a3s_boot";
const FLOW_QUEUE: &str = "cloud-operations";
const FLOW_TASK_RETRIES: u32 = 3;
const QUEUE_DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(5);
pub(crate) const CLOUD_FLOW_RUNTIME_BUILD_ID: &str = "a3s-cloud-workflows@1";

#[derive(Debug, thiserror::Error)]
pub enum FlowInfrastructureError {
    #[error("invalid PostgreSQL URL for A3S Flow: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("the PostgreSQL URL cannot define options because Cloud owns component search paths")]
    ConflictingOptions,
    #[error("could not initialize A3S Flow: {0}")]
    Flow(#[from] FlowError),
    #[error("could not initialize the A3S Boot Flow task queue: {0}")]
    Boot(#[from] BootError),
}

#[derive(Debug, thiserror::Error)]
pub enum FlowCoordinatorError {
    #[error("operation reconciliation failed: {0}")]
    Repository(#[from] crate::modules::shared_kernel::domain::RepositoryError),
    #[error("A3S Flow coordination failed: {0}")]
    Flow(#[from] FlowError),
    #[error("A3S Boot Flow task queue failed: {0}")]
    Boot(#[from] BootError),
    #[error("operation Flow queue drain timeout must be greater than zero")]
    InvalidDrainTimeout,
    #[error(
        "operation Flow queue did not drain before timeout (pending={pending}, active={active})"
    )]
    QueueDrainTimeout { pending: usize, active: usize },
    #[error("{count} operation Flow task(s) exhausted queue processing: {details}")]
    TerminalTaskFailures { count: usize, details: String },
    #[error("operation Flow cycle failed ({cycle}); queue shutdown also failed: {shutdown}")]
    CycleAndShutdown { cycle: String, shutdown: String },
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
    queue_backend: PostgresQueueBackend,
    queue: Arc<Queue>,
    task_manager: Arc<BootFlowTaskManager>,
}

#[derive(Clone)]
pub struct FlowRuntimeRouter {
    deployments: Arc<dyn FlowRuntime>,
    builds: Arc<dyn FlowRuntime>,
    executions: Arc<dyn FlowRuntime>,
    agent_executions: Arc<dyn FlowRuntime>,
    workflow_runs: Arc<dyn FlowRuntime>,
}

impl FlowRuntimeRouter {
    pub fn new(
        deployments: Arc<dyn FlowRuntime>,
        builds: Arc<dyn FlowRuntime>,
        executions: Arc<dyn FlowRuntime>,
        agent_executions: Arc<dyn FlowRuntime>,
        workflow_runs: Arc<dyn FlowRuntime>,
    ) -> Self {
        Self {
            deployments,
            builds,
            executions,
            agent_executions,
            workflow_runs,
        }
    }
}

#[async_trait::async_trait]
impl FlowRuntime for FlowRuntimeRouter {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        use crate::modules::agents::application::{
            AGENT_EXECUTION_WORKFLOW_NAME, AGENT_EXECUTION_WORKFLOW_VERSION,
        };
        use crate::modules::artifacts::application::{BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION};
        use crate::modules::executions::application::{
            EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
        };
        use crate::modules::workflow::{WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION};
        use crate::modules::workloads::infrastructure::{
            DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
            LEGACY_DEPLOYMENT_WORKFLOW_VERSION, PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
            PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION, PREVIOUS_DEPLOYMENT_WORKFLOW_VERSION,
            STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION,
        };

        let runtime = match (
            invocation.spec.name.as_str(),
            invocation.spec.version.as_str(),
        ) {
            (BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION) => &self.builds,
            (EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION) => &self.executions,
            (AGENT_EXECUTION_WORKFLOW_NAME, AGENT_EXECUTION_WORKFLOW_VERSION) => {
                &self.agent_executions
            }
            (WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION) => &self.workflow_runs,
            (DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)
            | (DEPLOYMENT_WORKFLOW_NAME, PREVIOUS_DEPLOYMENT_WORKFLOW_VERSION)
            | (DEPLOYMENT_WORKFLOW_NAME, LEGACY_DEPLOYMENT_WORKFLOW_VERSION)
            | (
                PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
                PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
            )
            | (STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION) => &self.deployments,
            _ => {
                return Err(FlowError::Runtime(format!(
                    "Cloud has no workflow runtime for {}@{}",
                    invocation.spec.name, invocation.spec.version
                )))
            }
        };
        runtime.run_workflow(invocation).await
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        if invocation.step_name.starts_with("build_") {
            self.builds.run_step(invocation).await
        } else if invocation.step_name.starts_with("agent_execution_") {
            self.agent_executions.run_step(invocation).await
        } else if invocation.step_name.starts_with("execution_") {
            self.executions.run_step(invocation).await
        } else if invocation.step_name.starts_with("workflow_run_") {
            self.workflow_runs.run_step(invocation).await
        } else {
            self.deployments.run_step(invocation).await
        }
    }
}

pub struct FlowOperationCoordinator {
    reconciler: crate::modules::operations::OperationReconciler,
    scheduler: FlowScheduler,
    queue_backend: PostgresQueueBackend,
    queue: Arc<Queue>,
    interval: Duration,
    drain_timeout: Duration,
}

struct FlowQueueCycle {
    report: FlowCoordinatorReport,
    stats: QueueStats,
}

impl FlowOperationCoordinator {
    pub fn new(
        reconciler: crate::modules::operations::OperationReconciler,
        flow: &FlowInfrastructure,
        interval: Duration,
        drain_timeout: Duration,
    ) -> Result<Self, FlowCoordinatorError> {
        if drain_timeout.is_zero() {
            return Err(FlowCoordinatorError::InvalidDrainTimeout);
        }
        Ok(Self {
            reconciler,
            scheduler: FlowScheduler::new(flow.engine(), flow.task_manager()),
            queue_backend: flow.queue_backend(),
            queue: flow.queue(),
            interval,
            drain_timeout,
        })
    }

    pub async fn run_once(&self) -> Result<FlowCoordinatorReport, FlowCoordinatorError> {
        let before_queue = self.queue_backend.stats_async().await?;
        self.queue.start(ModuleRef::new()).await?;
        let cycle = self.run_cycle(before_queue, true).await;
        let shutdown = self.queue.shutdown().await;
        match (cycle, shutdown) {
            (Ok(cycle), Ok(())) => Ok(cycle.report),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
            (Err(cycle), Err(shutdown)) => Err(FlowCoordinatorError::CycleAndShutdown {
                cycle: cycle.to_string(),
                shutdown: shutdown.to_string(),
            }),
        }
    }

    async fn run_cycle(
        &self,
        before_queue: QueueStats,
        drain_queue: bool,
    ) -> Result<FlowQueueCycle, FlowCoordinatorError> {
        let before = self.reconciler.run_once().await?;
        let tick = self.scheduler.enqueue_due_work(Utc::now()).await?;
        let after_queue = if drain_queue {
            self.drain_queue().await?
        } else {
            self.queue_backend.stats_async().await?
        };
        self.reject_new_terminal_failures(before_queue, after_queue)
            .await?;
        let after = self.reconciler.run_once().await?;
        Ok(FlowQueueCycle {
            report: FlowCoordinatorReport {
                reconciled_before_work: before.projected,
                reconciled_after_work: after.projected,
                reconciliation_failures: before.failures.len() + after.failures.len(),
                recovered_tasks: 0,
                enqueued_tasks: tick.enqueued_tasks,
                handled_tasks: after_queue.completed.saturating_sub(before_queue.completed),
            },
            stats: after_queue,
        })
    }

    async fn drain_queue(&self) -> Result<QueueStats, FlowCoordinatorError> {
        let drained = tokio::time::timeout(self.drain_timeout, async {
            loop {
                let stats = self.queue_backend.stats_async().await?;
                if stats.pending == 0 && stats.active == 0 {
                    return Ok::<QueueStats, BootError>(stats);
                }
                tokio::time::sleep(QUEUE_DRAIN_POLL_INTERVAL).await;
            }
        })
        .await;
        match drained {
            Ok(result) => Ok(result?),
            Err(_) => {
                let stats = self.queue_backend.stats_async().await?;
                Err(FlowCoordinatorError::QueueDrainTimeout {
                    pending: stats.pending,
                    active: stats.active,
                })
            }
        }
    }

    async fn reject_new_terminal_failures(
        &self,
        before: QueueStats,
        after: QueueStats,
    ) -> Result<(), FlowCoordinatorError> {
        let count = after.failed.saturating_sub(before.failed);
        if count == 0 {
            return Ok(());
        }
        let details = self
            .queue_backend
            .failures_async()
            .await?
            .into_iter()
            .skip(before.failed)
            .take(count)
            .map(|failure| format!("{} [{}]: {}", failure.id, failure.name, failure.message))
            .collect::<Vec<_>>()
            .join("; ");
        Err(FlowCoordinatorError::TerminalTaskFailures {
            count,
            details: if details.is_empty() {
                "failure records were not retained".to_string()
            } else {
                details
            },
        })
    }

    pub async fn run(
        self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), FlowCoordinatorError> {
        let mut queue_stats = self.queue_backend.stats_async().await?;
        self.queue.start(ModuleRef::new()).await?;
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
                    match self.run_cycle(queue_stats, false).await {
                        Ok(cycle) => {
                            queue_stats = cycle.stats;
                            let report = cycle.report;
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
                        Err(error) => {
                            tracing::error!(error = %error, "operation Flow cycle failed");
                            if let Ok(current) = self.queue_backend.stats_async().await {
                                queue_stats = current;
                            }
                        }
                    }
                }
            }
        }
        self.queue.shutdown().await?;
        Ok(())
    }
}

impl FlowInfrastructure {
    pub async fn connect(
        database_url: &str,
        runtime: Arc<dyn FlowRuntime>,
    ) -> Result<Self, FlowInfrastructureError> {
        Self::connect_with_queue_options(database_url, runtime, QueueOptions::new()).await
    }

    pub async fn connect_with_queue_options(
        database_url: &str,
        runtime: Arc<dyn FlowRuntime>,
        queue_options: QueueOptions,
    ) -> Result<Self, FlowInfrastructureError> {
        let flow_url = scoped_postgres_url(database_url, FLOW_SCHEMA)?;
        let boot_url = scoped_postgres_url(database_url, BOOT_SCHEMA)?;
        let store = Arc::new(PostgresEventStore::connect(flow_url.as_str()).await?);
        let engine = FlowEngine::builder(runtime)
            .with_store(store)
            .with_runtime_build_compatibility(cloud_runtime_build_compatibility()?)
            .build();
        let queue_backend =
            PostgresQueueBackend::connect(boot_url.as_str(), FLOW_QUEUE, queue_options).await?;
        let queue = Arc::new(Queue::new(FLOW_QUEUE, queue_backend.clone()));
        let task_policy = BootFlowTaskPolicy::new()
            .with_retry_policy(QueueRetryPolicy::fixed(
                FLOW_TASK_RETRIES,
                queue_options.poll_interval,
            ))
            .with_max_stalled_count(1)
            .with_deduplication(BootFlowTaskDeduplication::UntilTerminal);
        let task_manager = Arc::new(
            BootFlowTaskManager::new(engine.clone(), Arc::clone(&queue))
                .with_task_policy(task_policy)?,
        );
        task_manager.register()?;
        Ok(Self {
            engine,
            queue_backend,
            queue,
            task_manager,
        })
    }

    pub fn engine(&self) -> FlowEngine {
        self.engine.clone()
    }

    fn queue_backend(&self) -> PostgresQueueBackend {
        self.queue_backend.clone()
    }

    fn queue(&self) -> Arc<Queue> {
        Arc::clone(&self.queue)
    }

    fn task_manager(&self) -> Arc<BootFlowTaskManager> {
        Arc::clone(&self.task_manager)
    }

    pub async fn health(&self) -> HealthIndicatorResult {
        let runs = match self.engine.list_run_ids().await {
            Ok(runs) => runs.len(),
            Err(error) => {
                return HealthIndicatorResult::down().with_detail_value("error", error.to_string())
            }
        };
        let stats = match self.queue_backend.stats_async().await {
            Ok(stats) => stats,
            Err(error) => {
                return HealthIndicatorResult::down().with_detail_value("error", error.to_string())
            }
        };
        let worker_error = match self.queue_backend.last_worker_error() {
            Ok(error) => error,
            Err(error) => {
                return HealthIndicatorResult::down().with_detail_value("error", error.to_string())
            }
        };
        let health = if stats.failed > 0 || worker_error.is_some() {
            HealthIndicatorResult::down()
        } else {
            HealthIndicatorResult::up()
        }
        .with_detail_value("runs", runs)
        .with_detail_value("pendingTasks", stats.pending)
        .with_detail_value("activeTasks", stats.active)
        .with_detail_value("completedTasks", stats.completed)
        .with_detail_value("failedTasks", stats.failed);
        match worker_error {
            Some(error) => health.with_detail_value("workerError", error),
            None => health,
        }
    }
}

fn cloud_runtime_build_compatibility() -> Result<RuntimeBuildCompatibility, FlowError> {
    Ok(
        RuntimeBuildCompatibility::new(RuntimeBuildId::new(CLOUD_FLOW_RUNTIME_BUILD_ID)?)
            .accept_unpinned(),
    )
}

pub async fn connect_flow(
    database_url: &str,
    runtime: Arc<dyn FlowRuntime>,
    queue_options: QueueOptions,
) -> Result<FlowInfrastructure, FlowInfrastructureError> {
    let flow = FlowInfrastructure::connect_with_queue_options(database_url, runtime, queue_options)
        .await?;
    let retired_runs = retire_incompatible_build_workflows(&flow.engine()).await?;
    if retired_runs > 0 {
        tracing::warn!(
            retired_runs,
            "cancelled incompatible build workflow histories through A3S Flow"
        );
    }
    Ok(flow)
}

async fn retire_incompatible_build_workflows(engine: &FlowEngine) -> Result<usize, FlowError> {
    use crate::modules::artifacts::application::{
        BUILD_WORKFLOW_NAME, RETIRED_BUILD_WORKFLOW_VERSIONS,
    };

    let mut retired = 0usize;
    for snapshot in engine.list_snapshots().await? {
        if snapshot.spec.name != BUILD_WORKFLOW_NAME
            || !RETIRED_BUILD_WORKFLOW_VERSIONS.contains(&snapshot.spec.version.as_str())
            || snapshot.status.is_terminal()
        {
            continue;
        }
        engine
            .cancel(
                &snapshot.run_id,
                Some(format!(
                    "{}@{} predates the sole Box-native build workflow; rebuild with cloud.build@5",
                    snapshot.spec.name, snapshot.spec.version
                )),
            )
            .await?;
        retired += 1;
    }
    Ok(retired)
}

fn scoped_postgres_url(database_url: &str, schema: &str) -> Result<Url, FlowInfrastructureError> {
    let mut url = Url::parse(database_url)?;
    if url.query_pairs().any(|(key, _)| key == "options") {
        return Err(FlowInfrastructureError::ConflictingOptions);
    }
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_flow::{WorkflowRunStatus, WorkflowSpec};
    use chrono::{DateTime, Utc};
    use serde_json::json;

    #[test]
    fn runtime_build_policy_pins_current_runs_and_admits_legacy_histories() -> Result<(), FlowError>
    {
        let compatibility = cloud_runtime_build_compatibility()?;

        assert_eq!(
            compatibility.current_build_id().as_str(),
            CLOUD_FLOW_RUNTIME_BUILD_ID
        );
        assert!(compatibility.accepts_unpinned());
        assert!(compatibility.supports(Some(compatibility.current_build_id())));
        assert!(compatibility.supports(None));
        Ok(())
    }

    #[derive(Clone, Copy)]
    struct StubRuntime(&'static str);

    #[async_trait::async_trait]
    impl FlowRuntime for StubRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> Result<RuntimeCommand, FlowError> {
            Ok(invocation.context().complete(json!(self.0)))
        }

        async fn run_step(
            &self,
            _invocation: StepInvocation,
        ) -> Result<serde_json::Value, FlowError> {
            Ok(json!(self.0))
        }
    }

    #[derive(Clone, Copy)]
    struct SuspendingRuntime;

    #[async_trait::async_trait]
    impl FlowRuntime for SuspendingRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> Result<RuntimeCommand, FlowError> {
            let resume_at = "2099-01-01T00:00:00Z"
                .parse::<DateTime<Utc>>()
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            Ok(invocation
                .context()
                .wait_until("retirement-probe", resume_at))
        }

        async fn run_step(
            &self,
            _invocation: StepInvocation,
        ) -> Result<serde_json::Value, FlowError> {
            Err(FlowError::Runtime(
                "retirement probe does not execute steps".into(),
            ))
        }
    }

    fn workflow(name: &str, version: &str) -> WorkflowInvocation {
        WorkflowInvocation {
            run_id: "run-1".into(),
            spec: WorkflowSpec::rust_embedded(name, version, "cloud", "run"),
            input: json!({}),
            history: Vec::new(),
        }
    }

    fn step(name: &str) -> StepInvocation {
        StepInvocation {
            run_id: "run-1".into(),
            step_id: "step-1".into(),
            step_name: name.into(),
            input: json!({}),
            history: Vec::new(),
        }
    }

    fn router() -> FlowRuntimeRouter {
        FlowRuntimeRouter::new(
            Arc::new(StubRuntime("deployment")),
            Arc::new(StubRuntime("build")),
            Arc::new(StubRuntime("execution")),
            Arc::new(StubRuntime("agent_execution")),
            Arc::new(StubRuntime("workflow_run")),
        )
    }

    #[tokio::test]
    async fn runtime_router_preserves_all_production_workflow_identities() -> Result<(), FlowError>
    {
        for (name, version, expected) in [
            ("cloud.deployment", "1", "deployment"),
            ("cloud.deployment", "2", "deployment"),
            ("cloud.placement-group-deployment", "1", "deployment"),
            ("cloud.workload.stop", "1", "deployment"),
            ("cloud.build", "5", "build"),
            ("cloud.execution", "1", "execution"),
            ("cloud.agent-execution", "1", "agent_execution"),
            ("cloud.workflow-run", "1", "workflow_run"),
        ] {
            assert_eq!(
                router().run_workflow(workflow(name, version)).await?,
                RuntimeCommand::Complete {
                    output: json!(expected)
                }
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn runtime_router_rejects_retired_build_workflows() {
        for version in crate::modules::artifacts::application::RETIRED_BUILD_WORKFLOW_VERSIONS {
            let error = router()
                .run_workflow(workflow("cloud.build", version))
                .await
                .expect_err("retired build workflow must be rejected");
            assert_eq!(
                error.to_string(),
                format!("runtime error: Cloud has no workflow runtime for cloud.build@{version}")
            );
        }
    }

    #[tokio::test]
    async fn startup_retires_only_known_incompatible_build_histories() -> Result<(), FlowError> {
        let engine = FlowEngine::in_memory(Arc::new(SuspendingRuntime));
        for (run_id, name, version) in [
            ("legacy-build-1", "cloud.build", "1"),
            ("legacy-build-4", "cloud.build", "4"),
            ("current-build", "cloud.build", "5"),
            ("future-build", "cloud.build", "6"),
            ("deployment", "cloud.deployment", "1"),
        ] {
            engine
                .start_with_id(
                    run_id,
                    WorkflowSpec::rust_embedded(name, version, "cloud", "run"),
                    json!({}),
                )
                .await?;
        }

        assert_eq!(retire_incompatible_build_workflows(&engine).await?, 2);
        assert_eq!(retire_incompatible_build_workflows(&engine).await?, 0);

        for version in ["1", "4"] {
            let snapshot = engine.snapshot(&format!("legacy-build-{version}")).await?;
            assert_eq!(snapshot.status, WorkflowRunStatus::Cancelled);
            assert_eq!(
                snapshot.error.as_deref(),
                Some(
                    format!(
                        "cloud.build@{version} predates the sole Box-native build workflow; rebuild with cloud.build@5"
                    )
                    .as_str()
                )
            );
        }
        for run_id in ["current-build", "future-build", "deployment"] {
            assert_eq!(
                engine.snapshot(run_id).await?.status,
                WorkflowRunStatus::Suspended
            );
        }

        let due = engine.list_due_waits(DateTime::<Utc>::MAX_UTC).await?;
        assert_eq!(
            due,
            vec![
                ("current-build".into(), "retirement-probe".into()),
                ("deployment".into(), "retirement-probe".into()),
                ("future-build".into(), "retirement-probe".into()),
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn runtime_router_routes_build_steps_by_reserved_prefix() -> Result<(), FlowError> {
        assert_eq!(
            router().run_step(step("build_prepare_input")).await?,
            json!("build")
        );
        assert_eq!(
            router()
                .run_step(step("execution_schedule_runtime"))
                .await?,
            json!("execution")
        );
        assert_eq!(
            router().run_step(step("agent_execution_prepare")).await?,
            json!("agent_execution")
        );
        assert_eq!(
            router().run_step(step("resolve_deployment")).await?,
            json!("deployment")
        );
        assert_eq!(
            router().run_step(step("stop_workload_resolve")).await?,
            json!("deployment")
        );
        Ok(())
    }

    #[tokio::test]
    async fn runtime_router_rejects_unknown_workflow_identity() {
        let error = router()
            .run_workflow(workflow("cloud.unknown", "1"))
            .await
            .expect_err("unknown workflow must be rejected");
        assert_eq!(
            error.to_string(),
            "runtime error: Cloud has no workflow runtime for cloud.unknown@1"
        );
    }

    #[test]
    fn component_urls_own_isolated_search_paths() -> Result<(), FlowInfrastructureError> {
        let database_url = "postgres://user:secret@localhost/cloud?application_name=a3s";
        let flow_query = scoped_postgres_url(database_url, FLOW_SCHEMA)?
            .query()
            .unwrap_or_default()
            .to_string();
        assert!(flow_query.contains("application_name=a3s"));
        assert!(flow_query.contains("options=-csearch_path%3Da3s_flow"));
        let boot_query = scoped_postgres_url(database_url, BOOT_SCHEMA)?
            .query()
            .unwrap_or_default()
            .to_string();
        assert!(boot_query.contains("application_name=a3s"));
        assert!(boot_query.contains("options=-csearch_path%3Da3s_boot"));
        assert!(matches!(
            scoped_postgres_url(
                "postgres://localhost/cloud?options=-cfoo%3Dbar",
                FLOW_SCHEMA
            ),
            Err(FlowInfrastructureError::ConflictingOptions)
        ));
        Ok(())
    }
}
