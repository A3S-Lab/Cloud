use crate::artifact::CloudBoxArtifactPort;
use crate::secret::CloudBoxSecretMaterializer;
use crate::{BoxRuntimeConfig, BoxRuntimeIsolation, NodeRuntimeProvider};
use a3s_box_runtime::{
    BoxArtifactPort, BoxRuntimeDriver, BoxRuntimeDriverConfig, BoxSecretMaterializer,
    ExecutionIsolation,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeDriver, RuntimeResult,
    RuntimeStateStore,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Builds the Node Agent's sole Runtime client from the canonical A3S Box
/// configuration and the Agent-owned Runtime state root.
pub fn build_box_runtime_client(
    config: &BoxRuntimeConfig,
    state_root: impl AsRef<Path>,
) -> RuntimeResult<Arc<dyn RuntimeClient>> {
    Ok(build_box_runtime_provider(config, state_root)?.into_client())
}

/// Builds the production Box Runtime composition together with the one Cloud
/// Secret adapter that is bound after node enrollment.
pub fn build_box_runtime_provider(
    config: &BoxRuntimeConfig,
    state_root: impl AsRef<Path>,
) -> RuntimeResult<NodeRuntimeProvider> {
    let materializer = Arc::new(CloudBoxSecretMaterializer::new());
    let artifact_port = Arc::new(CloudBoxArtifactPort::new());
    let driver = Arc::new(build_box_runtime_driver(
        config,
        materializer.clone(),
        artifact_port.clone(),
    )?);
    let state: Arc<dyn RuntimeStateStore> =
        Arc::new(FileRuntimeStateStore::new(state_root.as_ref()));
    let driver: Arc<dyn RuntimeDriver> = driver;
    let client: Arc<dyn RuntimeClient> = Arc::new(ManagedRuntimeClient::new(state, driver));
    Ok(NodeRuntimeProvider::new(
        client,
        materializer,
        artifact_port,
    ))
}

fn build_box_runtime_driver(
    config: &BoxRuntimeConfig,
    materializer: Arc<CloudBoxSecretMaterializer>,
    artifact_port: Arc<CloudBoxArtifactPort>,
) -> RuntimeResult<BoxRuntimeDriver> {
    let driver = BoxRuntimeDriver::new_with_isolation(
        BoxRuntimeDriverConfig {
            home_dir: config.home_dir.clone(),
            secret_root: config.secret_root.clone(),
            control_timeout: Duration::from_millis(config.control_timeout_ms),
            task_poll_interval: Duration::from_millis(config.task_poll_interval_ms),
        },
        match config.isolation {
            BoxRuntimeIsolation::Microvm => ExecutionIsolation::Microvm,
            BoxRuntimeIsolation::Sandbox => ExecutionIsolation::Sandbox,
        },
    )?;
    let materializer: Arc<dyn BoxSecretMaterializer> = materializer;
    let artifact_port: Arc<dyn BoxArtifactPort> = artifact_port;
    Ok(driver
        .with_secret_materializer(materializer)
        .with_artifact_port(artifact_port))
}

#[cfg(test)]
mod tests;
