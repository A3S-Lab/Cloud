use crate::infrastructure::FlowOperationCoordinator;
use crate::modules::agents::AgentExecutionReconciler;
use crate::modules::artifacts::application::BuildRunReconciler;
use crate::modules::edge::{
    GatewayCertificateReconciler, GatewayReplicaRecoveryReconciler, GatewayRolloutReconciler,
    GatewayRolloutRollbackReconciler, McpCredentialDeliveryReceiptSweeper,
    McpGatewayDesiredStateReconciler, McpGatewaySnapshotReconciler,
};
use crate::modules::executions::ExecutionReconciler;
use crate::modules::fleet::{LogCompactionWorker, LogRetentionWorker, NodeControlServer};
use crate::modules::integration_events::OutboxRelay;
use crate::modules::notifications::A3sEventOutboundNotificationConsumer;
use crate::modules::sources::GithubConnectionAuthorityReconciler;
use crate::modules::workflow::{
    HumanTaskCoordinator, HumanTaskResumeWorker, WorkflowRunReconciler,
};
use crate::modules::workloads::{
    NodeDrainEvacuationReconciler, ReplicaDeploymentMaterializer, ReplicaRetirementReconciler,
    SecretRotationRestartReconciler, WorkloadRuntimeReconciler,
};
use a3s_boot::{BootApplication, BootError, BootRequest, BootResponse, HttpAdapter, Result};
use std::future::Future;
use std::net::SocketAddr;
use tokio::sync::watch;
use tokio::task::{JoinError, JoinSet};

pub struct ControlPlane {
    application: BootApplication,
    workers: ControlPlaneWorkers,
}

#[derive(Default)]
pub(crate) struct ControlPlaneWorkers {
    build_run_reconciler: Option<BuildRunReconciler>,
    execution_reconciler: Option<ExecutionReconciler>,
    agent_execution_reconciler: Option<AgentExecutionReconciler>,
    workflow_run_reconciler: Option<WorkflowRunReconciler>,
    human_task_coordinator: Option<HumanTaskCoordinator>,
    human_task_resume_worker: Option<HumanTaskResumeWorker>,
    github_authority_reconciler: Option<GithubConnectionAuthorityReconciler>,
    operation_coordinator: Option<FlowOperationCoordinator>,
    gateway_certificate_reconciler: Option<GatewayCertificateReconciler>,
    mcp_gateway_desired_state_reconciler: Option<McpGatewayDesiredStateReconciler>,
    mcp_gateway_snapshot_reconciler: Option<McpGatewaySnapshotReconciler>,
    mcp_credential_delivery_receipt_sweeper: Option<McpCredentialDeliveryReceiptSweeper>,
    gateway_rollout_reconciler: Option<GatewayRolloutReconciler>,
    gateway_replica_recovery_reconciler: Option<GatewayReplicaRecoveryReconciler>,
    gateway_rollout_rollback_reconciler: Option<GatewayRolloutRollbackReconciler>,
    secret_rotation_restart_reconciler: Option<SecretRotationRestartReconciler>,
    node_drain_evacuation_reconciler: Option<NodeDrainEvacuationReconciler>,
    replica_deployment_materializer: Option<ReplicaDeploymentMaterializer>,
    replica_retirement_reconciler: Option<ReplicaRetirementReconciler>,
    workload_reconciler: Option<WorkloadRuntimeReconciler>,
    log_retention_worker: Option<LogRetentionWorker>,
    log_compaction_worker: Option<LogCompactionWorker>,
    outbound_notification_consumer: Option<A3sEventOutboundNotificationConsumer>,
    outbox_relay: Option<OutboxRelay>,
    node_control_server: Option<NodeControlServer>,
}

impl ControlPlaneWorkers {
    pub(crate) fn relay(outbox_relay: OutboxRelay) -> Self {
        Self {
            outbox_relay: Some(outbox_relay),
            ..Self::default()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        build_run_reconciler: Option<BuildRunReconciler>,
        execution_reconciler: Option<ExecutionReconciler>,
        agent_execution_reconciler: Option<AgentExecutionReconciler>,
        workflow_run_reconciler: Option<WorkflowRunReconciler>,
        human_task_coordinator: Option<HumanTaskCoordinator>,
        human_task_resume_worker: Option<HumanTaskResumeWorker>,
        github_authority_reconciler: Option<GithubConnectionAuthorityReconciler>,
        operation_coordinator: Option<FlowOperationCoordinator>,
        gateway_certificate_reconciler: Option<GatewayCertificateReconciler>,
        mcp_gateway_desired_state_reconciler: Option<McpGatewayDesiredStateReconciler>,
        mcp_gateway_snapshot_reconciler: Option<McpGatewaySnapshotReconciler>,
        mcp_credential_delivery_receipt_sweeper: Option<McpCredentialDeliveryReceiptSweeper>,
        gateway_rollout_reconciler: Option<GatewayRolloutReconciler>,
        gateway_replica_recovery_reconciler: Option<GatewayReplicaRecoveryReconciler>,
        gateway_rollout_rollback_reconciler: Option<GatewayRolloutRollbackReconciler>,
        secret_rotation_restart_reconciler: Option<SecretRotationRestartReconciler>,
        node_drain_evacuation_reconciler: Option<NodeDrainEvacuationReconciler>,
        replica_deployment_materializer: Option<ReplicaDeploymentMaterializer>,
        replica_retirement_reconciler: Option<ReplicaRetirementReconciler>,
        workload_reconciler: Option<WorkloadRuntimeReconciler>,
        log_retention_worker: Option<LogRetentionWorker>,
        log_compaction_worker: Option<LogCompactionWorker>,
        outbound_notification_consumer: Option<A3sEventOutboundNotificationConsumer>,
        outbox_relay: Option<OutboxRelay>,
        node_control_server: Option<NodeControlServer>,
    ) -> Self {
        Self {
            build_run_reconciler,
            execution_reconciler,
            agent_execution_reconciler,
            workflow_run_reconciler,
            human_task_coordinator,
            human_task_resume_worker,
            github_authority_reconciler,
            operation_coordinator,
            gateway_certificate_reconciler,
            mcp_gateway_desired_state_reconciler,
            mcp_gateway_snapshot_reconciler,
            mcp_credential_delivery_receipt_sweeper,
            gateway_rollout_reconciler,
            gateway_replica_recovery_reconciler,
            gateway_rollout_rollback_reconciler,
            secret_rotation_restart_reconciler,
            node_drain_evacuation_reconciler,
            replica_deployment_materializer,
            replica_retirement_reconciler,
            workload_reconciler,
            log_retention_worker,
            log_compaction_worker,
            outbound_notification_consumer,
            outbox_relay,
            node_control_server,
        }
    }
}

#[derive(Debug)]
struct WorkerExit {
    name: &'static str,
    error: Option<String>,
    shutdown_requested: bool,
}

fn spawn_worker<Start, Worker>(
    workers: &mut JoinSet<WorkerExit>,
    name: &'static str,
    shutdown: watch::Receiver<bool>,
    start: Start,
) where
    Start: FnOnce(watch::Receiver<bool>) -> Worker + Send + 'static,
    Worker: Future<Output = ()> + Send + 'static,
{
    let lifecycle = shutdown.clone();
    workers.spawn(async move {
        start(shutdown).await;
        WorkerExit {
            name,
            error: None,
            shutdown_requested: *lifecycle.borrow(),
        }
    });
}

fn spawn_fallible_worker<Start, Worker, Error>(
    workers: &mut JoinSet<WorkerExit>,
    name: &'static str,
    shutdown: watch::Receiver<bool>,
    start: Start,
) where
    Start: FnOnce(watch::Receiver<bool>) -> Worker + Send + 'static,
    Worker: Future<Output = std::result::Result<(), Error>> + Send + 'static,
    Error: std::fmt::Display,
{
    let lifecycle = shutdown.clone();
    workers.spawn(async move {
        let error = start(shutdown).await.err().map(|error| error.to_string());
        WorkerExit {
            name,
            error,
            shutdown_requested: *lifecycle.borrow(),
        }
    });
}

fn unexpected_worker_exit(
    completed: Option<std::result::Result<WorkerExit, JoinError>>,
) -> BootError {
    match completed {
        Some(Ok(exit)) => worker_exit_error(exit),
        Some(Err(error)) => {
            BootError::Internal(format!("control-plane worker task failed: {error}"))
        }
        None => BootError::Internal("control-plane worker supervisor became empty".into()),
    }
}

fn worker_exit_error(exit: WorkerExit) -> BootError {
    match exit.error {
        Some(error) => BootError::Internal(format!(
            "control-plane worker {:?} failed: {error}",
            exit.name
        )),
        None => BootError::Internal(format!(
            "control-plane worker {:?} stopped before shutdown",
            exit.name
        )),
    }
}

async fn drain_workers(workers: &mut JoinSet<WorkerExit>) -> Option<BootError> {
    let mut worker_error = None;
    while let Some(completed) = workers.join_next().await {
        match completed {
            Ok(exit) if !exit.shutdown_requested => {
                worker_error.get_or_insert_with(|| worker_exit_error(exit));
            }
            Ok(_) => {}
            Err(error) => {
                worker_error.get_or_insert_with(|| {
                    BootError::Internal(format!("control-plane worker task failed: {error}"))
                });
            }
        }
    }
    worker_error
}

impl ControlPlane {
    pub(crate) fn new(application: BootApplication, workers: ControlPlaneWorkers) -> Self {
        Self {
            application,
            workers,
        }
    }

    pub async fn call(&self, request: BootRequest) -> Result<BootResponse> {
        self.application.call(request).await
    }

    pub async fn serve_with<A>(self, adapter: &A, address: SocketAddr) -> Result<()>
    where
        A: HttpAdapter,
    {
        let shutdown_application = self.application.clone();
        if let Err(error) = self.application.bootstrap().await {
            let _ = shutdown_application.shutdown().await;
            return Err(error);
        }
        let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
        let mut workers = JoinSet::new();
        if let Some(reconciler) = self.workers.build_run_reconciler {
            spawn_worker(
                &mut workers,
                "build-run reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.execution_reconciler {
            spawn_worker(
                &mut workers,
                "execution reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.agent_execution_reconciler {
            spawn_worker(
                &mut workers,
                "Agent execution reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.workflow_run_reconciler {
            spawn_worker(
                &mut workers,
                "WorkflowRun reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(coordinator) = self.workers.human_task_coordinator {
            spawn_worker(
                &mut workers,
                "HumanTask coordinator",
                shutdown_receiver.clone(),
                move |shutdown| coordinator.run(shutdown),
            );
        }
        if let Some(worker) = self.workers.human_task_resume_worker {
            spawn_worker(
                &mut workers,
                "HumanTask resume worker",
                shutdown_receiver.clone(),
                move |shutdown| worker.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.github_authority_reconciler {
            spawn_worker(
                &mut workers,
                "GitHub authority reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.gateway_certificate_reconciler {
            spawn_worker(
                &mut workers,
                "Gateway certificate reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.mcp_gateway_desired_state_reconciler {
            spawn_worker(
                &mut workers,
                "MCP Gateway desired-state reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.mcp_gateway_snapshot_reconciler {
            spawn_worker(
                &mut workers,
                "MCP Gateway snapshot reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(worker) = self.workers.mcp_credential_delivery_receipt_sweeper {
            spawn_worker(
                &mut workers,
                "MCP credential receipt sweeper",
                shutdown_receiver.clone(),
                move |shutdown| worker.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.gateway_rollout_reconciler {
            spawn_worker(
                &mut workers,
                "Gateway rollout reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.gateway_replica_recovery_reconciler {
            spawn_worker(
                &mut workers,
                "Gateway replica recovery reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.gateway_rollout_rollback_reconciler {
            spawn_worker(
                &mut workers,
                "Gateway rollout rollback reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.secret_rotation_restart_reconciler {
            spawn_worker(
                &mut workers,
                "Secret rotation restart reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.node_drain_evacuation_reconciler {
            spawn_worker(
                &mut workers,
                "node-drain evacuation reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(materializer) = self.workers.replica_deployment_materializer {
            spawn_worker(
                &mut workers,
                "replica deployment materializer",
                shutdown_receiver.clone(),
                move |shutdown| materializer.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.replica_retirement_reconciler {
            spawn_worker(
                &mut workers,
                "replica retirement reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(coordinator) = self.workers.operation_coordinator {
            spawn_fallible_worker(
                &mut workers,
                "Operation Flow coordinator",
                shutdown_receiver.clone(),
                move |shutdown| coordinator.run(shutdown),
            );
        }
        if let Some(reconciler) = self.workers.workload_reconciler {
            spawn_worker(
                &mut workers,
                "Workload Runtime reconciler",
                shutdown_receiver.clone(),
                move |shutdown| reconciler.run(shutdown),
            );
        }
        if let Some(worker) = self.workers.log_retention_worker {
            spawn_worker(
                &mut workers,
                "log retention worker",
                shutdown_receiver.clone(),
                move |shutdown| worker.run(shutdown),
            );
        }
        if let Some(worker) = self.workers.log_compaction_worker {
            spawn_worker(
                &mut workers,
                "log compaction worker",
                shutdown_receiver.clone(),
                move |shutdown| worker.run(shutdown),
            );
        }
        if let Some(consumer) = self.workers.outbound_notification_consumer {
            spawn_fallible_worker(
                &mut workers,
                "outbound notification A3S Event consumer",
                shutdown_receiver.clone(),
                move |shutdown| consumer.run(shutdown),
            );
        }
        if let Some(relay) = self.workers.outbox_relay {
            spawn_worker(
                &mut workers,
                "Outbox relay",
                shutdown_receiver.clone(),
                move |shutdown| relay.run(shutdown),
            );
        }
        if let Some(node_control) = self.workers.node_control_server {
            spawn_fallible_worker(
                &mut workers,
                "node-control listener",
                shutdown_receiver.clone(),
                move |shutdown| node_control.run(shutdown),
            );
        }
        let monitor_workers = !workers.is_empty();
        let serve_result = {
            let serve = adapter.serve(self.application, address);
            tokio::pin!(serve);
            tokio::select! {
                result = &mut serve => result,
                result = wait_for_shutdown_signal() => result,
                completed = workers.join_next(), if monitor_workers => {
                    Err(unexpected_worker_exit(completed))
                },
            }
        };
        let _ = shutdown_sender.send(true);
        let worker_error = drain_workers(&mut workers).await;
        let shutdown_result = shutdown_application.shutdown().await;

        match (serve_result, worker_error, shutdown_result) {
            (Err(error), _, _) => Err(error),
            (Ok(()), Some(error), _) => Err(error),
            (Ok(()), None, Err(error)) => Err(error),
            (Ok(()), None, Ok(())) => Ok(()),
        }
    }
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() -> Result<()> {
    let mut terminate =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|error| BootError::Internal(format!("could not register SIGTERM: {error}")))?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result.map_err(|error| BootError::Internal(format!("could not register SIGINT: {error}")))?;
            Ok(())
        }
        received = terminate.recv() => {
            received.ok_or_else(|| BootError::Internal("SIGTERM stream closed".into()))?;
            Ok(())
        }
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> Result<()> {
    tokio::signal::ctrl_c()
        .await
        .map_err(|error| BootError::Internal(format!("could not register Ctrl+C: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_has_no_per_worker_supervision_rail() {
        let production = include_str!("server.rs")
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production server source");
        for forbidden in [
            ["tokio", "::spawn("].concat(),
            ["unbounded_", "channel::<BootError>"].concat(),
            ["failure_", "sender"].concat(),
        ] {
            assert!(
                !production.contains(&forbidden),
                "server restored a per-worker supervision mechanism {forbidden:?}"
            );
        }
    }

    #[tokio::test]
    async fn supervisor_rejects_an_unexpected_clean_worker_exit() {
        let (_shutdown_sender, shutdown_receiver) = watch::channel(false);
        let mut workers = JoinSet::new();
        spawn_worker(
            &mut workers,
            "fixture worker",
            shutdown_receiver,
            |_| async {},
        );

        let error = unexpected_worker_exit(workers.join_next().await);
        assert!(
            error
                .to_string()
                .contains("fixture worker\" stopped before shutdown"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn supervisor_rejects_an_unexpected_worker_error() {
        let (_shutdown_sender, shutdown_receiver) = watch::channel(false);
        let mut workers = JoinSet::new();
        spawn_fallible_worker(
            &mut workers,
            "fallible fixture",
            shutdown_receiver,
            |_| async { Err::<(), _>("fixture failure") },
        );

        let error = unexpected_worker_exit(workers.join_next().await);
        let message = error.to_string();
        assert!(
            message.contains("fallible fixture") && message.contains("fixture failure"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn supervisor_accepts_workers_that_exit_after_requested_shutdown() {
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let mut workers = JoinSet::new();
        spawn_worker(
            &mut workers,
            "shutdown fixture",
            shutdown_receiver,
            |mut shutdown| async move {
                let _ = shutdown.changed().await;
            },
        );

        shutdown_sender.send(true).expect("request shutdown");
        assert!(drain_workers(&mut workers).await.is_none());
    }

    #[tokio::test]
    async fn supervisor_observes_worker_panics() {
        let mut workers = JoinSet::new();
        workers.spawn(async { panic!("fixture panic") });

        let error = drain_workers(&mut workers)
            .await
            .expect("worker panic must fail supervision");
        assert!(error.to_string().contains("worker task failed"), "{error}");
    }
}
