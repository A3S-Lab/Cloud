use crate::infrastructure::FlowOperationCoordinator;
use crate::modules::artifacts::application::BuildRunReconciler;
use crate::modules::edge::GatewayCertificateReconciler;
use crate::modules::fleet::{LogCompactionWorker, LogRetentionWorker, NodeControlServer};
use crate::modules::integration_events::OutboxRelay;
use crate::modules::workloads::{SecretRotationRestartReconciler, WorkloadRuntimeReconciler};
use a3s_boot::{BootApplication, BootError, BootRequest, BootResponse, HttpAdapter, Result};
use std::net::SocketAddr;

pub struct ControlPlane {
    application: BootApplication,
    workers: ControlPlaneWorkers,
}

pub(crate) struct ControlPlaneWorkers {
    build_run_reconciler: Option<BuildRunReconciler>,
    operation_coordinator: Option<FlowOperationCoordinator>,
    gateway_certificate_reconciler: Option<GatewayCertificateReconciler>,
    secret_rotation_restart_reconciler: Option<SecretRotationRestartReconciler>,
    workload_reconciler: Option<WorkloadRuntimeReconciler>,
    log_retention_worker: Option<LogRetentionWorker>,
    log_compaction_worker: Option<LogCompactionWorker>,
    outbox_relay: Option<OutboxRelay>,
    node_control_server: Option<NodeControlServer>,
}

impl ControlPlaneWorkers {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        build_run_reconciler: Option<BuildRunReconciler>,
        operation_coordinator: Option<FlowOperationCoordinator>,
        gateway_certificate_reconciler: Option<GatewayCertificateReconciler>,
        secret_rotation_restart_reconciler: Option<SecretRotationRestartReconciler>,
        workload_reconciler: Option<WorkloadRuntimeReconciler>,
        log_retention_worker: Option<LogRetentionWorker>,
        log_compaction_worker: Option<LogCompactionWorker>,
        outbox_relay: Option<OutboxRelay>,
        node_control_server: Option<NodeControlServer>,
    ) -> Self {
        Self {
            build_run_reconciler,
            operation_coordinator,
            gateway_certificate_reconciler,
            secret_rotation_restart_reconciler,
            workload_reconciler,
            log_retention_worker,
            log_compaction_worker,
            outbox_relay,
            node_control_server,
        }
    }
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
        let (failure_sender, mut failure_receiver) =
            tokio::sync::mpsc::unbounded_channel::<BootError>();
        let mut workers = Vec::new();
        if let Some(reconciler) = self.workers.build_run_reconciler {
            workers.push(tokio::spawn(reconciler.run(shutdown_receiver.clone())));
        }
        if let Some(reconciler) = self.workers.gateway_certificate_reconciler {
            workers.push(tokio::spawn(reconciler.run(shutdown_receiver.clone())));
        }
        if let Some(reconciler) = self.workers.secret_rotation_restart_reconciler {
            workers.push(tokio::spawn(reconciler.run(shutdown_receiver.clone())));
        }
        if let Some(coordinator) = self.workers.operation_coordinator {
            workers.push(tokio::spawn(coordinator.run(shutdown_receiver.clone())));
        }
        if let Some(reconciler) = self.workers.workload_reconciler {
            workers.push(tokio::spawn(reconciler.run(shutdown_receiver.clone())));
        }
        if let Some(worker) = self.workers.log_retention_worker {
            workers.push(tokio::spawn(worker.run(shutdown_receiver.clone())));
        }
        if let Some(worker) = self.workers.log_compaction_worker {
            workers.push(tokio::spawn(worker.run(shutdown_receiver.clone())));
        }
        if let Some(relay) = self.workers.outbox_relay {
            workers.push(tokio::spawn(relay.run(shutdown_receiver.clone())));
        }
        if let Some(node_control) = self.workers.node_control_server {
            let failure_sender = failure_sender.clone();
            let lifecycle = shutdown_receiver.clone();
            workers.push(tokio::spawn(async move {
                let result = node_control.run(shutdown_receiver).await;
                if !*lifecycle.borrow() {
                    let error = match result {
                        Ok(()) => BootError::Internal(
                            "node-control listener stopped before shutdown".into(),
                        ),
                        Err(error) => BootError::Internal(error.to_string()),
                    };
                    let _ = failure_sender.send(error);
                }
            }));
        }
        let serve_result = {
            let serve = adapter.serve(self.application, address);
            tokio::pin!(serve);
            tokio::select! {
                result = &mut serve => result,
                result = wait_for_shutdown_signal() => result,
                failure = failure_receiver.recv() => Err(failure.unwrap_or_else(|| {
                    BootError::Internal("control-plane background failure channel closed".into())
                })),
            }
        };
        let _ = shutdown_sender.send(true);
        let mut worker_error = None;
        for worker in workers {
            if let Err(error) = worker.await {
                worker_error.get_or_insert_with(|| {
                    BootError::Internal(format!("control-plane worker failed: {error}"))
                });
            }
        }
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
