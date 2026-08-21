mod steps;
mod v2;
mod workflow;

#[cfg(test)]
mod tests;

use super::object_namespace_access::{
    IObjectNamespaceAccessResolver, SharedObjectNamespaceAccessResolver,
};
use crate::infrastructure::flow_step_retry_policy;
use crate::modules::data::application::{
    ObjectNamespaceCredentialMaterializer, ObjectNamespaceRecoveryExecutor,
    LEGACY_OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
};
use crate::modules::data::domain::ObjectNamespaceError;
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation,
};
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
const OBJECT_NAMESPACE_SEAL_PAGE: &str = "object_namespace_seal_page";
const OBJECT_NAMESPACE_SEAL_VERIFY_PAGE: &str = "object_namespace_seal_verify_page";
const OBJECT_NAMESPACE_SEAL_FINALIZE: &str = "object_namespace_seal_finalize";
const OBJECT_NAMESPACE_RESTORE_PREFLIGHT_PAGE: &str = "object_namespace_restore_preflight_page";
const OBJECT_NAMESPACE_RESTORE_APPLY_PAGE: &str = "object_namespace_restore_apply_page";
const OBJECT_NAMESPACE_RESTORE_VERIFY_PAGE: &str = "object_namespace_restore_verify_page";
const OBJECT_NAMESPACE_RESTORE_FINALIZE: &str = "object_namespace_restore_finalize";
const OBJECT_NAMESPACE_DELETE_RETAINED_PREFLIGHT_PAGE: &str =
    "object_namespace_delete_retained_preflight_page";
const OBJECT_NAMESPACE_DELETE_SOURCE_PREFLIGHT_PAGE: &str =
    "object_namespace_delete_source_preflight_page";
const OBJECT_NAMESPACE_DELETE_MARK: &str = "object_namespace_delete_mark";
const OBJECT_NAMESPACE_DELETE_SOURCE_PAGE: &str = "object_namespace_delete_source_page";
const OBJECT_NAMESPACE_DELETE_SOURCE_ABSENCE: &str = "object_namespace_delete_source_absence";
const OBJECT_NAMESPACE_DELETE_RECOVERY_PLAN_PAGE: &str =
    "object_namespace_delete_recovery_plan_page";
const OBJECT_NAMESPACE_DELETE_RECOVERY_PAGE: &str = "object_namespace_delete_recovery_page";
const OBJECT_NAMESPACE_DELETE_RETAINED_POSTFLIGHT_PAGE: &str =
    "object_namespace_delete_retained_postflight_page";
const OBJECT_NAMESPACE_DELETE_RECOVERY_ANCHOR: &str = "object_namespace_delete_recovery_anchor";
const OBJECT_NAMESPACE_DELETE_FINALIZE: &str = "object_namespace_delete_finalize";
const STEP_NAMES: &[&str] = &[
    OBJECT_NAMESPACE_SEAL,
    OBJECT_NAMESPACE_RESTORE,
    OBJECT_NAMESPACE_DELETE,
    OBJECT_NAMESPACE_SEAL_PAGE,
    OBJECT_NAMESPACE_SEAL_VERIFY_PAGE,
    OBJECT_NAMESPACE_SEAL_FINALIZE,
    OBJECT_NAMESPACE_RESTORE_PREFLIGHT_PAGE,
    OBJECT_NAMESPACE_RESTORE_APPLY_PAGE,
    OBJECT_NAMESPACE_RESTORE_VERIFY_PAGE,
    OBJECT_NAMESPACE_RESTORE_FINALIZE,
    OBJECT_NAMESPACE_DELETE_RETAINED_PREFLIGHT_PAGE,
    OBJECT_NAMESPACE_DELETE_SOURCE_PREFLIGHT_PAGE,
    OBJECT_NAMESPACE_DELETE_MARK,
    OBJECT_NAMESPACE_DELETE_SOURCE_PAGE,
    OBJECT_NAMESPACE_DELETE_SOURCE_ABSENCE,
    OBJECT_NAMESPACE_DELETE_RECOVERY_PLAN_PAGE,
    OBJECT_NAMESPACE_DELETE_RECOVERY_PAGE,
    OBJECT_NAMESPACE_DELETE_RETAINED_POSTFLIGHT_PAGE,
    OBJECT_NAMESPACE_DELETE_RECOVERY_ANCHOR,
    OBJECT_NAMESPACE_DELETE_FINALIZE,
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

    fn retry_policy(&self, context: &WorkflowContext<'_>) -> a3s_flow::RetryPolicy {
        flow_step_retry_policy(context, self.retry_delay)
    }
}

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    STEP_NAMES.iter().copied()
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        LEGACY_OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
        crate::modules::data::application::OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
    ]
    .into_iter()
    .flat_map(|version| {
        [
            crate::modules::data::application::OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
            crate::modules::data::application::OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
            crate::modules::data::application::OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
        ]
        .into_iter()
        .map(move |name| (name, version))
    })
}

#[async_trait]
impl FlowRuntime for ObjectNamespaceRecoveryFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        match invocation.spec.version.as_str() {
            LEGACY_OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION => workflow::replay(self, invocation),
            crate::modules::data::application::OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION => {
                v2::replay(self, invocation)
            }
            _ => Err(unknown_workflow(&invocation)),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            OBJECT_NAMESPACE_SEAL => steps::seal(self, invocation.input_as()?).await,
            OBJECT_NAMESPACE_RESTORE => steps::restore(self, invocation.input_as()?).await,
            OBJECT_NAMESPACE_DELETE => steps::delete(self, invocation.input_as()?).await,
            _ => v2::run_step(self, invocation).await,
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

fn resolve<T: Serialize>(error: ObjectNamespaceError) -> a3s_flow::Result<serde_json::Value> {
    match error {
        ObjectNamespaceError::Unavailable(message) => Err(FlowError::Runtime(format!(
            "object namespace provider is temporarily unavailable: {message}"
        ))),
        ObjectNamespaceError::Invalid(message) => encode(RecoveryStepOutput::<T>::Rejected {
            reason: format!("invalid object namespace recovery request: {message}"),
        }),
        ObjectNamespaceError::Precondition(message) => encode(RecoveryStepOutput::<T>::Rejected {
            reason: format!("object namespace recovery precondition failed: {message}"),
        }),
        ObjectNamespaceError::Corrupt(message) => encode(RecoveryStepOutput::<T>::Rejected {
            reason: format!("object namespace recovery evidence is corrupt: {message}"),
        }),
        ObjectNamespaceError::Unsupported(message) => encode(RecoveryStepOutput::<T>::Rejected {
            reason: format!("object namespace provider is unsupported: {message}"),
        }),
    }
}

fn unknown_workflow(invocation: &WorkflowInvocation) -> FlowError {
    FlowError::Runtime(format!(
        "Cloud has no object namespace recovery workflow runtime for {}@{}",
        invocation.spec.name, invocation.spec.version
    ))
}
