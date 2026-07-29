use crate::BoxRuntimeConfig;
use a3s_box_runtime::{BoxRuntimeDriver, BoxRuntimeDriverConfig};
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
    let driver = Arc::new(BoxRuntimeDriver::new(BoxRuntimeDriverConfig {
        home_dir: config.home_dir.clone(),
        control_timeout: Duration::from_millis(config.control_timeout_ms),
        task_poll_interval: Duration::from_millis(config.task_poll_interval_ms),
    })?);
    let state: Arc<dyn RuntimeStateStore> =
        Arc::new(FileRuntimeStateStore::new(state_root.as_ref()));
    let driver: Arc<dyn RuntimeDriver> = driver;
    Ok(Arc::new(ManagedRuntimeClient::new(state, driver)))
}
