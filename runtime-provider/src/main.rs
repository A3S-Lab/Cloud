mod config;
mod driver;
mod module;

use std::path::PathBuf;
use std::sync::Arc;

use a3s_boot::{AxumAdapter, BootApplication, HealthModule};
use a3s_runtime::{FileRuntimeStateStore, ManagedRuntimeClient};
use config::ProviderAppConfig;
use driver::ProcessRuntimeDriver;
use module::RuntimeProviderModule;
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
    let config_path = std::env::var_os("A3S_RUNTIME_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config/runtime-provider.acl"));
    let config = ProviderAppConfig::from_acl_file(&config_path)?;
    let address = config.socket_addr()?;
    let body_limit = config.server.body_limit_bytes;
    let driver = Arc::new(ProcessRuntimeDriver::new(config.provider.clone())?);
    let state = Arc::new(FileRuntimeStateStore::new(&config.provider.state_path));
    let client = Arc::new(ManagedRuntimeClient::new(state, driver.clone()));
    let application = BootApplication::builder()
        .import(HealthModule::new("health").with_route("/health"))
        .import(RuntimeProviderModule::new(
            client,
            driver,
            config.provider.api_token.clone(),
        ))
        .build()?;
    info!(%address, provider = %config.provider.id, "starting development A3S Runtime provider");
    application
        .serve_with(&AxumAdapter::new().with_body_limit(body_limit), address)
        .await?;
    Ok(())
}
