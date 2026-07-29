use std::error::Error;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    use a3s_cloud_node_agent::{
        build_box_runtime_client, run_node_agent, NodeAgentConfig, NodeRuntimeProvider,
    };
    use tokio::sync::watch;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .try_init()?;
    let config_path = config_path()?;
    let config = NodeAgentConfig::load(config_path)?;
    let runtime =
        build_box_runtime_client(&config.box_runtime, config.node.state_dir.join("runtime"))?;
    let provider = NodeRuntimeProvider::new(runtime);
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

#[cfg(not(target_os = "linux"))]
fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("A3S Cloud Node Agent requires Linux because A3S Box is its sole Runtime provider".into())
}

#[cfg(target_os = "linux")]
fn config_path() -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let mut arguments = std::env::args_os();
    let executable = arguments
        .next()
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()))
        .unwrap_or_else(|| "a3s-cloud-node-agent".into());
    let Some(path) = arguments.next() else {
        return Err(format!("usage: {} <node-config.acl>", executable.to_string_lossy()).into());
    };
    if arguments.next().is_some() {
        return Err(format!("usage: {} <node-config.acl>", executable.to_string_lossy()).into());
    }
    Ok(path.into())
}

#[cfg(unix)]
#[cfg(target_os = "linux")]
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

#[cfg(all(target_os = "linux", not(unix)))]
async fn wait_for_shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "could not wait for interrupt signal");
    }
}
