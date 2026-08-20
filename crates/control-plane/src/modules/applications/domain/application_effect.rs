use crate::modules::shared_kernel::domain::{canonical_json_bounded, WorkflowRunId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const APPLICATION_WORKFLOW_EFFECT_MAX_BYTES: usize = 1_024;

/// Stable identity of one Applications-owned semantic effect emitted by an
/// exact Workflow run step attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationWorkflowEffect {
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub attempt: u32,
    pub ordinal: u32,
}

impl ApplicationWorkflowEffect {
    pub fn new(
        workflow_run_id: WorkflowRunId,
        step_id: impl Into<String>,
        attempt: u32,
        ordinal: u32,
    ) -> Result<Self, String> {
        let value = Self {
            workflow_run_id,
            step_id: step_id.into(),
            attempt,
            ordinal,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.workflow_run_id.as_uuid().is_nil()
            || self.attempt == 0
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(
                "Application Workflow effect requires an exact run, portable step, and positive attempt"
                    .into(),
            );
        }
        canonical_json_bounded(
            self,
            APPLICATION_WORKFLOW_EFFECT_MAX_BYTES,
            "Application Workflow effect",
        )?;
        Ok(())
    }

    pub(crate) fn deterministic_uuid(
        &self,
        namespace: Uuid,
        purpose: &str,
    ) -> Result<Uuid, String> {
        self.validate()?;
        if purpose.is_empty()
            || purpose.len() > 64
            || !purpose
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || matches!(byte, b'-' | b'_'))
        {
            return Err("Application Workflow effect purpose is invalid".into());
        }
        let mut identity = purpose.as_bytes().to_vec();
        identity.push(0);
        identity.extend(canonical_json_bounded(
            self,
            APPLICATION_WORKFLOW_EFFECT_MAX_BYTES,
            "Application Workflow effect identity",
        )?);
        Ok(Uuid::new_v5(&namespace, &identity))
    }
}
