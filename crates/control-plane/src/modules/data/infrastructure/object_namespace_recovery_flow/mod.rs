mod steps;
mod workflow;

#[cfg(test)]
mod tests;

use super::object_namespace_access::{
    IObjectNamespaceAccessResolver, SharedObjectNamespaceAccessResolver,
};
use crate::modules::data::application::{
    ObjectNamespaceCredentialMaterializer, ObjectNamespaceRecoveryExecutor,
};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

const MAXIMUM_OBJECTS: u32 = 4_096;
const MAXIMUM_OBJECT_BYTES: u64 = 64 * 1024 * 1024;
const MAXIMUM_STATE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAXIMUM_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
const RETRY_DELAY: Duration = Duration::from_secs(1);
const OBJECT_NAMESPACE_SEAL: &str = "object_namespace_seal";
const OBJECT_NAMESPACE_RESTORE: &str = "object_namespace_restore";
const OBJECT_NAMESPACE_DELETE: &str = "object_namespace_delete";
const STEP_NAMES: &[&str] = &[
    OBJECT_NAMESPACE_SEAL,
    OBJECT_NAMESPACE_RESTORE,
    OBJECT_NAMESPACE_DELETE,
];

#[derive(Clone)]
pub struct ObjectNamespaceRecoveryFlowRuntime {
    resolver: Arc<dyn IObjectNamespaceAccessResolver>,
    executor: ObjectNamespaceRecoveryExecutor,
    retry_delay: Duration,
}

impl ObjectNamespaceRecoveryFlowRuntime {
    pub fn new(credentials: ObjectNamespaceCredentialMaterializer) -> Result<Self, String> {
        Ok(Self {
            resolver: Arc::new(SharedObjectNamespaceAccessResolver::new(credentials)),
            executor: ObjectNamespaceRecoveryExecutor::new(
                MAXIMUM_OBJECTS,
                MAXIMUM_OBJECT_BYTES,
                MAXIMUM_STATE_BYTES,
                MAXIMUM_MANIFEST_BYTES,
            )?,
            retry_delay: RETRY_DELAY,
        })
    }

    #[cfg(test)]
    fn with_resolver(
        resolver: Arc<dyn IObjectNamespaceAccessResolver>,
        executor: ObjectNamespaceRecoveryExecutor,
    ) -> Self {
        Self {
            resolver,
            executor,
            retry_delay: Duration::from_millis(1),
        }
    }

    fn retry_policy(&self) -> a3s_flow::RetryPolicy {
        a3s_flow::RetryPolicy::fixed(u32::MAX, self.retry_delay)
    }
}

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    STEP_NAMES.iter().copied()
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        (
            crate::modules::data::application::OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
            crate::modules::data::application::OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
        ),
        (
            crate::modules::data::application::OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
            crate::modules::data::application::OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
        ),
        (
            crate::modules::data::application::OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
            crate::modules::data::application::OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
        ),
    ]
    .into_iter()
}

#[async_trait]
impl FlowRuntime for ObjectNamespaceRecoveryFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        workflow::replay(self, invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            OBJECT_NAMESPACE_SEAL => steps::seal(self, invocation.input_as()?).await,
            OBJECT_NAMESPACE_RESTORE => steps::restore(self, invocation.input_as()?).await,
            OBJECT_NAMESPACE_DELETE => steps::delete(self, invocation.input_as()?).await,
            step => Err(FlowError::Runtime(format!(
                "Cloud object namespace recovery workflow has no step {step:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum RecoveryStepOutput<T> {
    Completed { output: T },
    Rejected { reason: String },
}

fn encode<T: Serialize>(value: T) -> a3s_flow::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(FlowError::from)
}
