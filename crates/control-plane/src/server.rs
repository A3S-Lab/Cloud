use crate::modules::integration_events::OutboxRelay;
use crate::modules::operations::OperationReconciler;
use a3s_boot::{BootApplication, BootError, BootRequest, BootResponse, HttpAdapter, Result};
use std::net::SocketAddr;

pub struct ControlPlane {
    application: BootApplication,
    operation_reconciler: Option<OperationReconciler>,
    outbox_relay: Option<OutboxRelay>,
}

impl ControlPlane {
    pub(crate) fn new(
        application: BootApplication,
        operation_reconciler: Option<OperationReconciler>,
        outbox_relay: Option<OutboxRelay>,
    ) -> Self {
        Self {
            application,
            operation_reconciler,
            outbox_relay,
        }
    }

    pub async fn call(&self, request: BootRequest) -> Result<BootResponse> {
        self.application.call(request).await
    }

    pub async fn serve_with<A>(self, adapter: &A, address: SocketAddr) -> Result<()>
    where
        A: HttpAdapter,
    {
        let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
        let mut workers = Vec::new();
        if let Some(reconciler) = self.operation_reconciler {
            workers.push(tokio::spawn(reconciler.run(shutdown_receiver.clone())));
        }
        if let Some(relay) = self.outbox_relay {
            workers.push(tokio::spawn(relay.run(shutdown_receiver)));
        }
        let serve_result = serve_until_shutdown(self.application, adapter, address).await;
        let _ = shutdown_sender.send(true);
        let mut worker_error = None;
        for worker in workers {
            if let Err(error) = worker.await {
                worker_error.get_or_insert_with(|| {
                    BootError::Internal(format!("control-plane worker failed: {error}"))
                });
            }
        }
        serve_result?;
        worker_error.map_or(Ok(()), Err)
    }
}

async fn serve_until_shutdown<A>(
    application: BootApplication,
    adapter: &A,
    address: SocketAddr,
) -> Result<()>
where
    A: HttpAdapter,
{
    let shutdown_application = application.clone();
    if let Err(error) = application.bootstrap().await {
        let _ = shutdown_application.shutdown().await;
        return Err(error);
    }

    let serve = adapter.serve(application, address);
    tokio::pin!(serve);
    let serve_result = tokio::select! {
        result = &mut serve => result,
        result = wait_for_shutdown_signal() => {
            result?;
            Ok(())
        },
    };
    let shutdown_result = shutdown_application.shutdown().await;

    match (serve_result, shutdown_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
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
