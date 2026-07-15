use a3s_cloud_node_agent::{
    run_node_agent, DockerRuntimeDriver, NodeAgentConfig, NodeRuntimeBinding, NodeRuntimeProvider,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeDriver, RuntimeStateStore,
};
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .try_init()?;
    let config_path = config_path()?;
    let config = NodeAgentConfig::load(config_path)?;
    let driver = Arc::new(DockerRuntimeDriver::connect(&config.docker)?);
    let state: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(
        config.node.state_dir.join("runtime"),
    ));
    let runtime_driver: Arc<dyn RuntimeDriver> = driver.clone();
    let runtime: Arc<dyn RuntimeClient> =
        Arc::new(ManagedRuntimeClient::new(state, runtime_driver));
    let binding: Arc<dyn NodeRuntimeBinding> = driver;
    let provider = NodeRuntimeProvider::new(runtime, binding);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let signal = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });
    let result = run_node_agent(config, provider, shutdown_rx).await;
    signal.abort();
    result?;
    Ok(())
}

fn config_path() -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let mut arguments = std::env::args_os();
    let executable = arguments
        .next()
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()))
        .unwrap_or_else(|| "a3s-cloud-node-agent".into());
    let Some(path) = arguments.next() else {
        return Err(format!("usage: {} <node-config.hcl>", executable.to_string_lossy()).into());
    };
    if arguments.next().is_some() {
        return Err(format!("usage: {} <node-config.hcl>", executable.to_string_lossy()).into());
    }
    Ok(path.into())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "could not install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(error) = result {
                tracing::error!(%error, "could not wait for interrupt signal");
            }
        }
        () = terminate => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "could not wait for interrupt signal");
    }
}
