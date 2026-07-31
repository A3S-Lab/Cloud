use std::path::PathBuf;
use std::time::Duration;

use a3s_flow::{FlowScheduler, FlowWorker};
use a3s_workflow_server::{build_application, AppConfig};
use chrono::Utc;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .try_init()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let config_path = std::env::var_os("A3S_WORKFLOW_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config/workflow.acl"));
    let config = AppConfig::from_acl_file(&config_path)?;
    let worker_poll = Duration::from_millis(config.flow.worker_poll_ms);
    let scheduler_poll = Duration::from_millis(config.flow.scheduler_poll_ms);
    let lease_seconds = config.flow.inflight_lease_seconds;
    let services = build_application(config).await?;
    let queue: std::sync::Arc<dyn a3s_flow::FlowTaskQueue> = services.queue.clone();
    let worker = FlowWorker::new(services.engine.clone(), queue.clone());
    let scheduler = FlowScheduler::new(services.engine.clone(), queue);

    let recovered = services
        .queue
        .requeue_inflight_older_than(Utc::now() - chrono::Duration::seconds(lease_seconds))
        .await?;
    info!(recovered, "starting PostgreSQL-backed A3S Flow worker");

    let scheduler_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(scheduler_poll);
        loop {
            interval.tick().await;
            match scheduler.enqueue_due_work(Utc::now()).await {
                Ok(tick) if tick.enqueued_tasks > 0 => {
                    info!(tasks = tick.enqueued_tasks, "scheduled due workflow work");
                }
                Ok(_) => {}
                Err(error) => error!(%error, "workflow scheduler tick failed"),
            }
        }
    });

    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal?;
                info!("workflow worker received shutdown signal");
                scheduler_task.abort();
                break;
            }
            result = worker.run_once() => {
                match result {
                    Ok(Some(outcome)) => info!(?outcome.task, "workflow task completed"),
                    Ok(None) => tokio::time::sleep(worker_poll).await,
                    Err(error) => {
                        warn!(%error, "workflow task failed; lease will be recovered after expiry");
                        tokio::time::sleep(worker_poll).await;
                    }
                }
            }
        }
    }
    Ok(())
}
