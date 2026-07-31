use std::path::PathBuf;

use a3s_boot::AxumAdapter;
use a3s_workflow_server::{build_application, AppConfig};
use tracing::info;
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
    let address = config.socket_addr()?;
    let body_limit = config.server.body_limit_bytes;
    let services = build_application(config).await?;

    info!(%address, config = %config_path.display(), "starting A3S Workflow API");
    services
        .application
        .serve_with(&AxumAdapter::new().with_body_limit(body_limit), address)
        .await?;
    Ok(())
}
