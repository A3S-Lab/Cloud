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
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use url::Url;

pub(crate) const FLOW_SCHEMA: &str = "a3s_flow";
pub(crate) const BOOT_SCHEMA: &str = "a3s_boot";
const FLOW_QUEUE: &str = "cloud-operations";
const FLOW_TASK_RETRIES: u32 = 3;
const QUEUE_DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(5);
pub(crate) const CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID: &str = "a3s-cloud-workflows@5";
pub(crate) const REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS: &[&str] = &[
    "a3s-cloud-workflows@1",
    "a3s-cloud-workflows@2",
    "a3s-cloud-workflows@3",
    "a3s-cloud-workflows@4",
];

#[derive(Debug, thiserror::Error)]
pub enum FlowInfrastructureError {
    #[error("invalid PostgreSQL URL for A3S Flow: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("the PostgreSQL URL cannot define options because Cloud owns component search paths")]
    ConflictingOptions,
    #[error("could not initialize A3S Flow: {0}")]
    Flow(#[from] FlowError),
    #[error("invalid Cloud Flow runtime registry: {0}")]
    RuntimeRegistry(#[from] FlowRuntimeRegistryError),
    #[error("could not initialize the A3S Boot Flow task queue: {0}")]
    Boot(#[from] BootError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FlowRuntimeRegistryError {
    #[error("the Flow runtime registry cannot be empty")]
    EmptyRegistry,
    #[error("a Flow runtime owner cannot be empty")]
    EmptyOwner,
    #[error("Flow runtime {owner:?} must own at least one workflow identity")]
    MissingWorkflowIdentity { owner: String },
    #[error("Flow runtime {owner:?} has invalid workflow identity {name:?}@{version:?}")]
    InvalidWorkflowIdentity {
        owner: String,
        name: String,
        version: String,
    },
    #[error("Flow runtime {owner:?} has invalid step name {step_name:?}")]
    InvalidStepName { owner: String, step_name: String },
    #[error(
        "workflow identity {name}@{version} is owned by both {first_owner:?} and {conflicting_owner:?}"
    )]
    DuplicateWorkflowIdentity {
        name: String,
        version: String,
        first_owner: String,
        conflicting_owner: String,
    },
    #[error("step name {step_name:?} is owned by both {first_owner:?} and {conflicting_owner:?}")]
    DuplicateStepName {
        step_name: String,
        first_owner: String,
        conflicting_owner: String,
    },
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

/// Query-side access to the durable A3S Flow event store.
///
/// Management API processes need to inspect workflow history, but they do not
/// own workflow runtimes, the Boot task queue, or task execution. Keeping this
/// adapter separate makes that ownership impossible to acquire accidentally.
#[derive(Clone)]
pub struct FlowReadInfrastructure {
    engine: FlowEngine,
}

#[derive(Debug)]
struct ReadOnlyFlowRuntime;

#[async_trait::async_trait]
impl FlowRuntime for ReadOnlyFlowRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        Err(FlowError::Runtime(
            "the management Flow reader cannot execute workflows".into(),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        Err(FlowError::Runtime(
            "the management Flow reader cannot execute steps".into(),
        ))
    }
}

/// Exact process-level dispatch for every Cloud-owned A3S Flow runtime.
///
/// `StepInvocation` carries a step name but no workflow identity, so step names
/// are deliberately unique across the complete process registry.
#[derive(Clone)]
pub struct FlowRuntimeRouter {
    workflow_runtimes: Arc<BTreeMap<&'static str, BTreeMap<&'static str, RegisteredFlowRuntime>>>,
    step_runtimes: Arc<BTreeMap<&'static str, RegisteredFlowRuntime>>,
}

#[derive(Clone)]
struct RegisteredFlowRuntime {
    owner: &'static str,
    runtime: Arc<dyn FlowRuntime>,
}

struct FlowRuntimeRegistration {
    owner: &'static str,
    runtime: Arc<dyn FlowRuntime>,
    workflows: Vec<(&'static str, &'static str)>,
    steps: Vec<&'static str>,
}

impl FlowRuntimeRegistration {
    fn new(
        owner: &'static str,
        runtime: Arc<dyn FlowRuntime>,
        workflows: impl IntoIterator<Item = (&'static str, &'static str)>,
        steps: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            owner,
            runtime,
            workflows: workflows.into_iter().collect(),
            steps: steps.into_iter().collect(),
        }
    }
}

impl FlowRuntimeRouter {
    pub fn new(
        deployments: Arc<crate::modules::workloads::infrastructure::DeploymentFlowRuntime>,
        builds: Arc<crate::modules::artifacts::infrastructure::BuildFlowRuntime>,
        executions: Arc<crate::modules::executions::infrastructure::ExecutionFlowRuntime>,
        agent_executions: Arc<crate::modules::agents::infrastructure::AgentExecutionFlowRuntime>,
        workflow_runs: Arc<crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime>,
        object_namespace_recovery: Arc<crate::modules::data::ObjectNamespaceRecoveryFlowRuntime>,
    ) -> Result<Self, FlowInfrastructureError> {
        Self::from_registrations(production_runtime_registrations(
            deployments,
            builds,
            executions,
            agent_executions,
            workflow_runs,
            object_namespace_recovery,
        ))
        .map_err(Into::into)
    }

    fn from_registrations(
        registrations: impl IntoIterator<Item = FlowRuntimeRegistration>,
    ) -> Result<Self, FlowRuntimeRegistryError> {
        let mut workflow_runtimes =
            BTreeMap::<&'static str, BTreeMap<&'static str, RegisteredFlowRuntime>>::new();
        let mut step_runtimes = BTreeMap::<&'static str, RegisteredFlowRuntime>::new();
        let mut registration_count = 0usize;

        for registration in registrations {
            registration_count += 1;
            validate_registration(&registration)?;
            let registered = RegisteredFlowRuntime {
                owner: registration.owner,
                runtime: registration.runtime,
            };
            for (name, version) in registration.workflows {
                let versions = workflow_runtimes.entry(name).or_default();
                if let Some(existing) = versions.get(version) {
                    return Err(FlowRuntimeRegistryError::DuplicateWorkflowIdentity {
                        name: name.into(),
                        version: version.into(),
                        first_owner: existing.owner.into(),
                        conflicting_owner: registered.owner.into(),
                    });
                }
                versions.insert(version, registered.clone());
            }
            for step_name in registration.steps {
                if let Some(existing) = step_runtimes.get(step_name) {
                    return Err(FlowRuntimeRegistryError::DuplicateStepName {
                        step_name: step_name.into(),
                        first_owner: existing.owner.into(),
                        conflicting_owner: registered.owner.into(),
                    });
                }
                step_runtimes.insert(step_name, registered.clone());
            }
        }

        if registration_count == 0 {
            return Err(FlowRuntimeRegistryError::EmptyRegistry);
        }
        Ok(Self {
            workflow_runtimes: Arc::new(workflow_runtimes),
            step_runtimes: Arc::new(step_runtimes),
        })
    }
}

fn validate_registration(
    registration: &FlowRuntimeRegistration,
) -> Result<(), FlowRuntimeRegistryError> {
    if registration.owner.trim().is_empty() {
        return Err(FlowRuntimeRegistryError::EmptyOwner);
    }
    if registration.workflows.is_empty() {
        return Err(FlowRuntimeRegistryError::MissingWorkflowIdentity {
            owner: registration.owner.into(),
        });
    }
    for (name, version) in &registration.workflows {
        if name.trim().is_empty() || version.trim().is_empty() {
            return Err(FlowRuntimeRegistryError::InvalidWorkflowIdentity {
                owner: registration.owner.into(),
                name: (*name).into(),
                version: (*version).into(),
            });
        }
    }
    for step_name in &registration.steps {
        if step_name.trim().is_empty() {
            return Err(FlowRuntimeRegistryError::InvalidStepName {
                owner: registration.owner.into(),
                step_name: (*step_name).into(),
            });
        }
    }
    Ok(())
}

fn production_runtime_registrations(
    deployments: Arc<dyn FlowRuntime>,
    builds: Arc<dyn FlowRuntime>,
    executions: Arc<dyn FlowRuntime>,
    agent_executions: Arc<dyn FlowRuntime>,
    workflow_runs: Arc<dyn FlowRuntime>,
    object_namespace_recovery: Arc<dyn FlowRuntime>,
) -> Vec<FlowRuntimeRegistration> {
    use crate::modules::agents::infrastructure::{
        agent_execution_flow_step_names, agent_execution_flow_workflow_identities,
    };
    use crate::modules::artifacts::infrastructure::{
        build_flow_step_names, build_flow_workflow_identities,
    };
    use crate::modules::data::{
        object_namespace_recovery_flow_step_names,
        object_namespace_recovery_flow_workflow_identities,
    };
    use crate::modules::executions::infrastructure::{
        execution_flow_step_names, execution_flow_workflow_identities,
    };
    use crate::modules::workflow::infrastructure::{
        workflow_run_flow_step_names, workflow_run_flow_workflow_identities,
    };
    use crate::modules::workloads::infrastructure::{
        deployment_flow_step_names, deployment_flow_workflow_identities,
    };

    vec![
        FlowRuntimeRegistration::new(
            "workloads.deployment",
            deployments,
            deployment_flow_workflow_identities(),
            deployment_flow_step_names(),
        ),
        FlowRuntimeRegistration::new(
            "artifacts.build",
            builds,
            build_flow_workflow_identities(),
            build_flow_step_names(),
        ),
        FlowRuntimeRegistration::new(
            "executions",
            executions,
            execution_flow_workflow_identities(),
            execution_flow_step_names(),
        ),
        FlowRuntimeRegistration::new(
            "agents.execution",
            agent_executions,
            agent_execution_flow_workflow_identities(),
            agent_execution_flow_step_names(),
        ),
        FlowRuntimeRegistration::new(
            "workflow.runs",
            workflow_runs,
            workflow_run_flow_workflow_identities(),
            workflow_run_flow_step_names(),
        ),
        FlowRuntimeRegistration::new(
            "data.object-namespace-recovery",
            object_namespace_recovery,
            object_namespace_recovery_flow_workflow_identities(),
            object_namespace_recovery_flow_step_names(),
        ),
    ]
}

#[async_trait::async_trait]
impl FlowRuntime for FlowRuntimeRouter {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        let runtime = self
            .workflow_runtimes
            .get(invocation.spec.name.as_str())
            .and_then(|versions| versions.get(invocation.spec.version.as_str()))
            .ok_or_else(|| {
                FlowError::Runtime(format!(
                    "Cloud has no workflow runtime for {}@{}",
                    invocation.spec.name, invocation.spec.version
                ))
            })?;
        runtime.runtime.run_workflow(invocation).await
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        let runtime = self
            .step_runtimes
            .get(invocation.step_name.as_str())
            .ok_or_else(|| {
                FlowError::Runtime(format!(
                    "Cloud has no step runtime for {:?}",
                    invocation.step_name
                ))
            })?;
        runtime.runtime.run_step(invocation).await
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
        let store = Arc::new(PostgresEventStore::connect_verified(flow_url.as_str()).await?);
        let engine = FlowEngine::builder(runtime)
            .with_store(store)
            .with_runtime_build_compatibility(cloud_runtime_build_compatibility()?)
            .build();
        let queue_backend =
            PostgresQueueBackend::connect_verified(boot_url.as_str(), FLOW_QUEUE, queue_options)
                .await?;
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
        let runtime_build = match self.engine.runtime_build_compatibility() {
            Some(runtime_build) => runtime_build,
            None => {
                return HealthIndicatorResult::down()
                    .with_detail_value("error", "Flow runtime build policy is not configured")
            }
        };
        let current_runtime_build_id = runtime_build.current_build_id().as_str().to_owned();
        let compatible_runtime_build_ids = runtime_build
            .compatible_build_ids()
            .map(|build_id| build_id.as_str().to_owned())
            .collect::<Vec<_>>();
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
        .with_detail_value("currentRuntimeBuildId", current_runtime_build_id)
        .with_detail_value("compatibleRuntimeBuildIds", compatible_runtime_build_ids)
        .with_detail_value(
            "acceptsUnpinnedRuntimeBuilds",
            runtime_build.accepts_unpinned(),
        )
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

impl FlowReadInfrastructure {
    pub async fn connect(database_url: &str) -> Result<Self, FlowInfrastructureError> {
        let flow_url = scoped_postgres_url(database_url, FLOW_SCHEMA)?;
        let store = Arc::new(PostgresEventStore::connect_verified(flow_url.as_str()).await?);
        let engine = FlowEngine::builder(Arc::new(ReadOnlyFlowRuntime))
            .with_store(store)
            .with_runtime_build_compatibility(cloud_runtime_build_compatibility()?)
            .build();
        Ok(Self { engine })
    }

    pub fn engine(&self) -> FlowEngine {
        self.engine.clone()
    }

    pub async fn health(&self) -> HealthIndicatorResult {
        match self.engine.list_run_ids().await {
            Ok(runs) => HealthIndicatorResult::up().with_detail_value("runs", runs.len()),
            Err(error) => {
                HealthIndicatorResult::down().with_detail_value("error", error.to_string())
            }
        }
    }
}

pub(crate) fn cloud_runtime_build_compatibility() -> Result<RuntimeBuildCompatibility, FlowError> {
    let mut compatibility =
        RuntimeBuildCompatibility::new(RuntimeBuildId::new(CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID)?);
    for build_id in REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS {
        compatibility = compatibility.with_compatible_build(RuntimeBuildId::new(*build_id)?);
    }

    // Histories created before runtime-build pinning remain admitted only as
    // migration debt. Cloud never creates a new unpinned Operation run.
    Ok(compatibility.accept_unpinned())
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

pub(crate) fn scoped_postgres_url(
    database_url: &str,
    schema: &str,
) -> Result<Url, FlowInfrastructureError> {
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
    fn runtime_build_policy_pins_one_current_generation_and_admits_only_declared_legacy_builds(
    ) -> Result<(), FlowError> {
        let compatibility = cloud_runtime_build_compatibility()?;

        assert_eq!(
            compatibility.current_build_id().as_str(),
            CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID
        );
        let compatible_build_ids = compatibility
            .compatible_build_ids()
            .map(RuntimeBuildId::as_str)
            .collect::<Vec<_>>();
        let declared_build_ids = REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS
            .iter()
            .copied()
            .chain(std::iter::once(CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            compatible_build_ids,
            declared_build_ids.iter().copied().collect::<Vec<_>>()
        );
        assert_eq!(
            declared_build_ids.len(),
            REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS.len() + 1,
            "current and replay-compatible runtime build identities must be unique"
        );
        assert!(compatibility.accepts_unpinned());
        assert!(compatibility.supports(Some(compatibility.current_build_id())));
        assert!(compatibility.supports(Some(&RuntimeBuildId::new(
            REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS[0]
        )?)));
        assert!(!compatibility.supports(Some(&RuntimeBuildId::new("a3s-cloud-workflows@unknown")?)));
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
        WorkflowInvocation::new(
            "run-1",
            WorkflowSpec::rust_embedded(name, version, "cloud", "run"),
            json!({}),
            Vec::new(),
        )
    }

    fn step(name: &str) -> StepInvocation {
        StepInvocation::new("run-1", "step-1", name, json!({}), Vec::new())
    }

    fn router() -> FlowRuntimeRouter {
        FlowRuntimeRouter::from_registrations(production_runtime_registrations(
            Arc::new(StubRuntime("deployment")),
            Arc::new(StubRuntime("build")),
            Arc::new(StubRuntime("execution")),
            Arc::new(StubRuntime("agent_execution")),
            Arc::new(StubRuntime("workflow_run")),
            Arc::new(StubRuntime("object_namespace_recovery")),
        ))
        .expect("production Flow runtime registry must be valid")
    }

    #[tokio::test]
    async fn runtime_router_preserves_all_production_workflow_identities() -> Result<(), FlowError>
    {
        for (name, version, expected) in [
            ("cloud.deployment", "1", "deployment"),
            ("cloud.deployment", "2", "deployment"),
            ("cloud.deployment", "3", "deployment"),
            ("cloud.deployment", "4", "deployment"),
            ("cloud.placement-group-deployment", "1", "deployment"),
            ("cloud.placement-group-deployment", "2", "deployment"),
            ("cloud.workload.stop", "1", "deployment"),
            ("cloud.build", "5", "build"),
            ("cloud.execution", "1", "execution"),
            ("cloud.agent-execution", "1", "agent_execution"),
            ("cloud.workflow-run", "1", "workflow_run"),
            ("cloud.workflow-run", "2", "workflow_run"),
            ("cloud.workflow-run", "3", "workflow_run"),
            ("cloud.workflow-run", "4", "workflow_run"),
            (
                "cloud.object-namespace.seal",
                "1",
                "object_namespace_recovery",
            ),
            (
                "cloud.object-namespace.restore",
                "1",
                "object_namespace_recovery",
            ),
            (
                "cloud.object-namespace.delete",
                "1",
                "object_namespace_recovery",
            ),
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
    async fn runtime_router_routes_every_registered_step_to_its_exact_owner(
    ) -> Result<(), FlowError> {
        async fn assert_routes(
            router: &FlowRuntimeRouter,
            step_names: impl IntoIterator<Item = &'static str>,
            expected: &str,
        ) -> Result<(), FlowError> {
            for step_name in step_names {
                assert_eq!(
                    router.run_step(step(step_name)).await?,
                    json!(expected),
                    "step {step_name:?} routed to the wrong runtime"
                );
            }
            Ok(())
        }

        let router = router();
        assert_routes(
            &router,
            crate::modules::workloads::infrastructure::deployment_flow_step_names(),
            "deployment",
        )
        .await?;
        assert_routes(
            &router,
            crate::modules::artifacts::infrastructure::build_flow_step_names(),
            "build",
        )
        .await?;
        assert_routes(
            &router,
            crate::modules::executions::infrastructure::execution_flow_step_names(),
            "execution",
        )
        .await?;
        assert_routes(
            &router,
            crate::modules::agents::infrastructure::agent_execution_flow_step_names(),
            "agent_execution",
        )
        .await?;
        assert_routes(
            &router,
            crate::modules::workflow::infrastructure::workflow_run_flow_step_names(),
            "workflow_run",
        )
        .await?;
        assert_routes(
            &router,
            crate::modules::data::object_namespace_recovery_flow_step_names(),
            "object_namespace_recovery",
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn runtime_router_rejects_unknown_steps_instead_of_using_a_prefix_or_default_owner() {
        for step_name in [
            "build_future_step",
            "execution_future_step",
            "agent_execution_future_step",
            "workflow_run_future_step",
            "object_namespace_future_step",
            "unscoped_future_step",
        ] {
            let error = router()
                .run_step(step(step_name))
                .await
                .expect_err("unregistered step must be rejected");
            assert_eq!(
                error.to_string(),
                format!("runtime error: Cloud has no step runtime for {step_name:?}")
            );
        }
    }

    #[test]
    fn runtime_registry_rejects_workflow_identity_collisions_at_startup() {
        let error = FlowRuntimeRouter::from_registrations([
            FlowRuntimeRegistration::new(
                "first",
                Arc::new(StubRuntime("first")),
                [("cloud.shared", "1")],
                ["first_step"],
            ),
            FlowRuntimeRegistration::new(
                "second",
                Arc::new(StubRuntime("second")),
                [("cloud.shared", "1")],
                ["second_step"],
            ),
        ])
        .err()
        .expect("duplicate workflow identity must fail registry construction");
        assert_eq!(
            error,
            FlowRuntimeRegistryError::DuplicateWorkflowIdentity {
                name: "cloud.shared".into(),
                version: "1".into(),
                first_owner: "first".into(),
                conflicting_owner: "second".into(),
            }
        );
    }

    #[test]
    fn runtime_registry_rejects_step_name_collisions_at_startup() {
        let error = FlowRuntimeRouter::from_registrations([
            FlowRuntimeRegistration::new(
                "first",
                Arc::new(StubRuntime("first")),
                [("cloud.first", "1")],
                ["shared_step"],
            ),
            FlowRuntimeRegistration::new(
                "second",
                Arc::new(StubRuntime("second")),
                [("cloud.second", "1")],
                ["shared_step"],
            ),
        ])
        .err()
        .expect("duplicate step name must fail registry construction");
        assert_eq!(
            error,
            FlowRuntimeRegistryError::DuplicateStepName {
                step_name: "shared_step".into(),
                first_owner: "first".into(),
                conflicting_owner: "second".into(),
            }
        );
    }

    #[test]
    fn runtime_registry_rejects_empty_registration_metadata_at_startup() {
        let empty = FlowRuntimeRouter::from_registrations(std::iter::empty())
            .err()
            .expect("empty registry must fail");
        assert_eq!(empty, FlowRuntimeRegistryError::EmptyRegistry);

        let empty_owner = FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
            "",
            Arc::new(StubRuntime("empty-owner")),
            [("cloud.valid", "1")],
            std::iter::empty(),
        )])
        .err()
        .expect("empty owner must fail");
        assert_eq!(empty_owner, FlowRuntimeRegistryError::EmptyOwner);

        let missing_workflow =
            FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
                "owner",
                Arc::new(StubRuntime("missing-workflow")),
                std::iter::empty(),
                ["valid_step"],
            )])
            .err()
            .expect("missing workflow identity must fail");
        assert_eq!(
            missing_workflow,
            FlowRuntimeRegistryError::MissingWorkflowIdentity {
                owner: "owner".into(),
            }
        );

        for (name, version) in [("", "1"), ("cloud.valid", "")] {
            let invalid = FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
                "owner",
                Arc::new(StubRuntime("invalid-workflow")),
                [(name, version)],
                ["valid_step"],
            )])
            .err()
            .expect("empty workflow identity component must fail");
            assert_eq!(
                invalid,
                FlowRuntimeRegistryError::InvalidWorkflowIdentity {
                    owner: "owner".into(),
                    name: name.into(),
                    version: version.into(),
                }
            );
        }

        let empty_step = FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
            "owner",
            Arc::new(StubRuntime("empty-step")),
            [("cloud.valid", "1")],
            [""],
        )])
        .err()
        .expect("empty step name must fail");
        assert_eq!(
            empty_step,
            FlowRuntimeRegistryError::InvalidStepName {
                owner: "owner".into(),
                step_name: String::new(),
            }
        );
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
