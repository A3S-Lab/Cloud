use crate::{BoxRuntimeConfig, BoxRuntimeIsolation};
use a3s_box_runtime::{BoxRuntimeDriver, BoxRuntimeDriverConfig, ExecutionIsolation};
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
    let driver = Arc::new(build_box_runtime_driver(config)?);
    let state: Arc<dyn RuntimeStateStore> =
        Arc::new(FileRuntimeStateStore::new(state_root.as_ref()));
    let driver: Arc<dyn RuntimeDriver> = driver;
    Ok(Arc::new(ManagedRuntimeClient::new(state, driver)))
}

fn build_box_runtime_driver(config: &BoxRuntimeConfig) -> RuntimeResult<BoxRuntimeDriver> {
    BoxRuntimeDriver::new_with_isolation(
        BoxRuntimeDriverConfig {
            home_dir: config.home_dir.clone(),
            control_timeout: Duration::from_millis(config.control_timeout_ms),
            task_poll_interval: Duration::from_millis(config.task_poll_interval_ms),
        },
        match config.isolation {
            BoxRuntimeIsolation::Microvm => ExecutionIsolation::Microvm,
            BoxRuntimeIsolation::Sandbox => ExecutionIsolation::Sandbox,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(isolation: BoxRuntimeIsolation) -> (tempfile::TempDir, BoxRuntimeConfig) {
        let home = tempfile::tempdir().expect("temporary Box home");
        let config = BoxRuntimeConfig {
            home_dir: home.path().to_path_buf(),
            isolation,
            control_timeout_ms: 60_000,
            task_poll_interval_ms: 50,
        };
        (home, config)
    }

    #[test]
    fn selects_the_exact_configured_box_isolation_without_fallback() {
        for (configured, expected) in [
            (BoxRuntimeIsolation::Microvm, ExecutionIsolation::Microvm),
            (BoxRuntimeIsolation::Sandbox, ExecutionIsolation::Sandbox),
        ] {
            let (_home, config) = config(configured);
            let driver = build_box_runtime_driver(&config).expect("Box Runtime driver");

            assert_eq!(driver.execution_isolation(), expected);
        }
    }
}
