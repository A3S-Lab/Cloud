use super::{
    descriptor_failure_output, CapabilityType, ResolvedWorkflowRunStep, WorkflowExecutionOutcome,
    WorkflowExecutionStepOutput, WorkflowStepKind, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, Sha256Digest};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA: &str = "cloud.workflow.step-failure.v1";
pub const WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V2: &str = "cloud.workflow.step-failure.v2";
pub const WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V3: &str = "cloud.workflow.step-failure.v3";
pub const WORKFLOW_STEP_DEFAULT_OUTPUT_EVIDENCE_SCHEMA: &str =
    "cloud.workflow.step-default-output.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepFailureClassification {
    DispatchRejected,
    ExecutionFailed,
    ExecutionCancelled,
    ProviderRejected,
    ProviderAttemptsExhausted,
    ProviderIndeterminate,
    ProviderObservationLimit,
    ProviderResponseInvalid,
    ApplicationInvalid,
    ApplicationNotFound,
    ApplicationConflict,
    ApplicationForbidden,
}

impl WorkflowStepFailureClassification {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DispatchRejected => "dispatch_rejected",
            Self::ExecutionFailed => "execution_failed",
            Self::ExecutionCancelled => "execution_cancelled",
            Self::ProviderRejected => "provider_rejected",
            Self::ProviderAttemptsExhausted => "provider_attempts_exhausted",
            Self::ProviderIndeterminate => "provider_indeterminate",
            Self::ProviderObservationLimit => "provider_observation_limit",
            Self::ProviderResponseInvalid => "provider_response_invalid",
            Self::ApplicationInvalid => "application_invalid",
            Self::ApplicationNotFound => "application_not_found",
            Self::ApplicationConflict => "application_conflict",
            Self::ApplicationForbidden => "application_forbidden",
        }
    }

    const fn is_provider(self) -> bool {
        matches!(
            self,
            Self::ProviderRejected
                | Self::ProviderAttemptsExhausted
                | Self::ProviderIndeterminate
                | Self::ProviderObservationLimit
                | Self::ProviderResponseInvalid
        )
    }

    pub(crate) const fn is_application(self) -> bool {
        matches!(
            self,
            Self::ApplicationInvalid
                | Self::ApplicationNotFound
                | Self::ApplicationConflict
                | Self::ApplicationForbidden
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowStepFailureDetails {
    Execution { output: WorkflowExecutionStepOutput },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowStepFailureOutput {
    pub schema: String,
    pub step_id: String,
    pub classification: WorkflowStepFailureClassification,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<WorkflowStepFailureDetails>,
}

impl WorkflowStepFailureOutput {
    pub fn dispatch_rejected(
        step: &ResolvedWorkflowRunStep,
        message: String,
    ) -> Result<Self, String> {
        let value = Self::observe_dispatch_rejected(step, message)?;
        value.validate(step)?;
        Ok(value)
    }

    pub(crate) fn observe_dispatch_rejected(
        step: &ResolvedWorkflowRunStep,
        message: String,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA.into(),
            step_id: step.plan.id.clone(),
            classification: WorkflowStepFailureClassification::DispatchRejected,
            message,
            details: None,
        };
        value.validate_observation(step)?;
        Ok(value)
    }

    pub fn from_execution(
        step: &ResolvedWorkflowRunStep,
        output: WorkflowExecutionStepOutput,
    ) -> Result<Self, String> {
        let value = Self::observe_execution(step, output)?;
        value.validate(step)?;
        Ok(value)
    }

    pub(crate) fn observe_execution(
        step: &ResolvedWorkflowRunStep,
        output: WorkflowExecutionStepOutput,
    ) -> Result<Self, String> {
        let (classification, message) = match &output.outcome {
            WorkflowExecutionOutcome::Succeeded { .. } => {
                return Err("successful Workflow execution cannot produce failure output".into())
            }
            WorkflowExecutionOutcome::Failed { reason, .. } => (
                WorkflowStepFailureClassification::ExecutionFailed,
                reason.clone(),
            ),
            WorkflowExecutionOutcome::Cancelled => (
                WorkflowStepFailureClassification::ExecutionCancelled,
                "child Execution was cancelled".into(),
            ),
        };
        let value = Self {
            schema: WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA.into(),
            step_id: step.plan.id.clone(),
            classification,
            message,
            details: Some(WorkflowStepFailureDetails::Execution { output }),
        };
        value.validate_observation(step)?;
        Ok(value)
    }

    pub(crate) fn provider(
        step: &ResolvedWorkflowRunStep,
        classification: WorkflowStepFailureClassification,
        message: String,
    ) -> Result<Self, String> {
        if !classification.is_provider() {
            return Err("Workflow provider failure classification is invalid".into());
        }
        let value = Self {
            schema: WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V2.into(),
            step_id: step.plan.id.clone(),
            classification,
            message,
            details: None,
        };
        value.validate(step)?;
        Ok(value)
    }

    pub(crate) fn application_variable(
        step: &ResolvedWorkflowRunStep,
        classification: WorkflowStepFailureClassification,
    ) -> Result<Self, String> {
        let message = application_failure_message(classification)
            .ok_or_else(|| "Workflow Application failure classification is invalid".to_owned())?;
        let value = Self {
            schema: WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V3.into(),
            step_id: step.plan.id.clone(),
            classification,
            message: message.into(),
            details: None,
        };
        value.validate(step)?;
        Ok(value)
    }

    pub fn validate(&self, step: &ResolvedWorkflowRunStep) -> Result<(), String> {
        self.validate_observation(step)?;
        let failure =
            step.plan.failure.as_ref().ok_or_else(|| {
                "Workflow step failure output has no immutable contract".to_owned()
            })?;
        let error_output = descriptor_failure_output(failure)?;
        let encoded = serde_json::to_value(self)
            .map_err(|error| format!("Workflow step failure output is invalid: {error}"))?;
        if !error_output.value_type.matches_json_value(&encoded) {
            return Err("Workflow step failure output does not match its descriptor type".into());
        }
        Ok(())
    }

    pub(crate) fn validate_observation(
        &self,
        step: &ResolvedWorkflowRunStep,
    ) -> Result<(), String> {
        if self.step_id != step.plan.id {
            return Err("Workflow step failure output identity or message is invalid".into());
        }
        self.validate_shape()?;
        if self.classification.is_provider() {
            if !is_connector_step(step) {
                return Err("Workflow provider failure requires an exact Connector step".into());
            }
            return Ok(());
        }
        if self.classification.is_application() {
            if !is_application_variable_step(step) {
                return Err(
                    "Workflow Application failure requires the exact variable assignment step"
                        .into(),
                );
            }
            return Ok(());
        }
        if step.plan.kind != WorkflowStepKind::Execution {
            return Err("Workflow execution failure requires an Execution step".into());
        }
        if let Some(WorkflowStepFailureDetails::Execution { output }) = self.details.as_ref() {
            validate_failure_execution_authority(output, step)?;
        }
        Ok(())
    }

    pub(crate) fn validate_shape(&self) -> Result<(), String> {
        if super::validation::validate_identifier("Workflow failure step", &self.step_id).is_err()
            || self.message.is_empty()
            || self.message.len() > 16 * 1024
            || self.message.contains(['\0', '\r', '\n'])
        {
            return Err("Workflow step failure output identity or message is invalid".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow step failure output",
        )?;
        match (
            self.schema.as_str(),
            self.classification,
            self.details.as_ref(),
        ) {
            (
                WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA,
                WorkflowStepFailureClassification::DispatchRejected,
                None,
            ) => Ok(()),
            (
                WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA,
                WorkflowStepFailureClassification::ExecutionFailed,
                Some(WorkflowStepFailureDetails::Execution { output }),
            ) => {
                output.validate_shape()?;
                match &output.outcome {
                    WorkflowExecutionOutcome::Failed { reason, .. } if reason == &self.message => {
                        Ok(())
                    }
                    _ => Err("Workflow execution failure classification drifted".into()),
                }
            }
            (
                WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA,
                WorkflowStepFailureClassification::ExecutionCancelled,
                Some(WorkflowStepFailureDetails::Execution { output }),
            ) => {
                output.validate_shape()?;
                if output.outcome == WorkflowExecutionOutcome::Cancelled
                    && self.message == "child Execution was cancelled"
                {
                    Ok(())
                } else {
                    Err("Workflow execution cancellation classification drifted".into())
                }
            }
            (WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V2, classification, None)
                if classification.is_provider() =>
            {
                Ok(())
            }
            (WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V3, classification, None)
                if application_failure_message(classification) == Some(self.message.as_str()) =>
            {
                Ok(())
            }
            _ => Err("Workflow step failure details do not match their classification".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowStepDefaultOutputEvidence {
    pub schema: String,
    pub policy_digest: Sha256Digest,
    pub port: String,
    pub failure: WorkflowStepFailureOutput,
}

impl WorkflowStepDefaultOutputEvidence {
    pub(crate) fn new(
        step: &ResolvedWorkflowRunStep,
        failure: WorkflowStepFailureOutput,
    ) -> Result<Self, String> {
        let policy_digest = step.plan.policy_digest.clone().ok_or_else(|| {
            "Workflow default-output evidence lost its exact policy digest".to_owned()
        })?;
        let port = step
            .policy
            .as_ref()
            .and_then(|policy| policy.default_output.as_ref())
            .map(|output| output.port.clone())
            .ok_or_else(|| {
                "Workflow default-output evidence lost its immutable material".to_owned()
            })?;
        let value = Self {
            schema: WORKFLOW_STEP_DEFAULT_OUTPUT_EVIDENCE_SCHEMA.into(),
            policy_digest,
            port,
            failure,
        };
        value.validate(step)?;
        Ok(value)
    }

    pub fn validate(&self, step: &ResolvedWorkflowRunStep) -> Result<(), String> {
        let contract = step.plan.default_output.as_ref().ok_or_else(|| {
            "Workflow default-output evidence has no immutable Plan contract".to_owned()
        })?;
        let policy_digest = step.plan.policy_digest.as_ref().ok_or_else(|| {
            "Workflow default-output evidence has no exact policy digest".to_owned()
        })?;
        let material = step
            .policy
            .as_ref()
            .and_then(|policy| policy.default_output.as_ref())
            .ok_or_else(|| "Workflow default-output evidence has no policy material".to_owned())?;
        if self.schema != WORKFLOW_STEP_DEFAULT_OUTPUT_EVIDENCE_SCHEMA
            || step.plan.kind != WorkflowStepKind::Execution
            || &self.policy_digest != policy_digest
            || self.port != contract.output_port.name
            || self.port != material.port
        {
            return Err("Workflow default-output evidence authority drifted".into());
        }
        self.failure.validate_observation(step)?;
        canonical_json_bounded(
            self,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow default-output evidence",
        )?;
        Ok(())
    }

    pub(crate) fn validate_projection_shape(&self, step_id: &str) -> Result<(), String> {
        if self.schema != WORKFLOW_STEP_DEFAULT_OUTPUT_EVIDENCE_SCHEMA
            || self.failure.step_id != step_id
        {
            return Err("Workflow default-output projection evidence identity drifted".into());
        }
        super::validation::validate_identifier("Workflow default-output port", &self.port)?;
        self.failure.validate_shape()?;
        canonical_json_bounded(
            self,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow default-output projection evidence",
        )?;
        Ok(())
    }
}

fn is_connector_step(step: &ResolvedWorkflowRunStep) -> bool {
    step.plan.kind == WorkflowStepKind::Service
        && step.plan.capability.as_ref().is_some_and(|capability| {
            capability.capability_type == CapabilityType::ConnectorRevision
        })
}

fn is_application_variable_step(step: &ResolvedWorkflowRunStep) -> bool {
    step.plan.kind == WorkflowStepKind::Service
        && step.plan.capability.is_none()
        && step.plan.descriptor.as_ref().is_some_and(|descriptor| {
            descriptor.descriptor_id == "application.conversation-variable-assign"
        })
}

fn application_failure_message(
    classification: WorkflowStepFailureClassification,
) -> Option<&'static str> {
    match classification {
        WorkflowStepFailureClassification::ApplicationInvalid => {
            Some("Application variable assignment was rejected as invalid")
        }
        WorkflowStepFailureClassification::ApplicationNotFound => {
            Some("Application variable assignment authority was not found")
        }
        WorkflowStepFailureClassification::ApplicationConflict => {
            Some("Application variable assignment conflicted with current state")
        }
        WorkflowStepFailureClassification::ApplicationForbidden => {
            Some("Application variable assignment was forbidden")
        }
        _ => None,
    }
}

fn validate_failure_execution_authority(
    output: &WorkflowExecutionStepOutput,
    step: &ResolvedWorkflowRunStep,
) -> Result<(), String> {
    output.validate_shape()?;
    let capability = step
        .plan
        .capability
        .as_ref()
        .ok_or_else(|| "Workflow Execution failure lost its capability".to_owned())?;
    capability.validate()?;
    if capability.capability_type != CapabilityType::ExecutionTemplate
        || output.execution_template_id.as_uuid() != capability.resource_id
        || output.execution_template_revision_id.to_string() != capability.revision
        || output.execution_template_digest != capability.digest
    {
        return Err("Workflow Execution failure detail authority drifted".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::test_support::{
        routed_application_variable_workflow_run_input, routed_connector_workflow_run_input,
        routed_execution_workflow_run_input, TEST_APPLICATION_VARIABLE_STEP_ID,
        TEST_CONNECTOR_STEP_ID, TEST_EXECUTION_STEP_ID,
    };

    #[test]
    fn application_variable_failures_are_redacted_exact_v3_owner_observations() {
        let input = routed_application_variable_workflow_run_input()
            .expect("routed Application variable WorkflowRun input");
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == TEST_APPLICATION_VARIABLE_STEP_ID)
            .expect("Application variable step");
        let cases = [
            (
                WorkflowStepFailureClassification::ApplicationInvalid,
                "Application variable assignment was rejected as invalid",
            ),
            (
                WorkflowStepFailureClassification::ApplicationNotFound,
                "Application variable assignment authority was not found",
            ),
            (
                WorkflowStepFailureClassification::ApplicationConflict,
                "Application variable assignment conflicted with current state",
            ),
            (
                WorkflowStepFailureClassification::ApplicationForbidden,
                "Application variable assignment was forbidden",
            ),
        ];
        for (classification, message) in cases {
            let failure = WorkflowStepFailureOutput::application_variable(&step, classification)
                .expect("v3 Application variable failure");
            assert_eq!(failure.schema, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V3);
            assert_eq!(failure.classification, classification);
            assert_eq!(failure.message, message);
            assert!(failure.details.is_none());
            failure.validate(&step).expect("valid v3 failure");
        }

        assert!(WorkflowStepFailureOutput::application_variable(
            &step,
            WorkflowStepFailureClassification::ProviderRejected,
        )
        .is_err());

        let mut forged = WorkflowStepFailureOutput::application_variable(
            &step,
            WorkflowStepFailureClassification::ApplicationConflict,
        )
        .expect("v3 Application variable conflict");
        forged.message = "raw owner conflict: private session state".into();
        assert!(forged.validate(&step).is_err());
    }

    #[test]
    fn execution_failure_values_remain_on_the_v1_contract() {
        let input =
            routed_execution_workflow_run_input().expect("routed Execution WorkflowRun input");
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == TEST_EXECUTION_STEP_ID)
            .expect("Execution step");
        let failure = WorkflowStepFailureOutput::dispatch_rejected(
            &step,
            "Execution dispatch rejected: unavailable".into(),
        )
        .expect("v1 Execution failure");

        assert_eq!(failure.schema, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA);
        assert_eq!(
            failure.classification,
            WorkflowStepFailureClassification::DispatchRejected
        );
        assert!(failure.details.is_none());
        failure.validate(&step).expect("valid v1 failure");
        let encoded = serde_json::to_value(&failure).expect("encoded failure");
        assert!(encoded.get("details").is_none());
        assert_eq!(
            serde_json::from_value::<WorkflowStepFailureOutput>(encoded).expect("decoded failure"),
            failure
        );
    }

    #[test]
    fn connector_failure_values_are_exact_v2_provider_observations() {
        let input =
            routed_connector_workflow_run_input().expect("routed Connector WorkflowRun input");
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == TEST_CONNECTOR_STEP_ID)
            .expect("Connector step");
        let failure = WorkflowStepFailureOutput::provider(
            &step,
            WorkflowStepFailureClassification::ProviderIndeterminate,
            "provider outcome is indeterminate".into(),
        )
        .expect("v2 Connector failure");

        assert_eq!(failure.schema, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V2);
        assert!(failure.details.is_none());
        failure.validate(&step).expect("valid v2 failure");
        let encoded = serde_json::to_value(&failure).expect("encoded failure");
        assert!(encoded.get("details").is_none());
        assert_eq!(
            serde_json::from_value::<WorkflowStepFailureOutput>(encoded).expect("decoded failure"),
            failure
        );

        let mut wrong_schema = failure.clone();
        wrong_schema.schema = WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA.into();
        assert!(wrong_schema.validate(&step).is_err());

        let mut wrong_classification = failure.clone();
        wrong_classification.classification = WorkflowStepFailureClassification::DispatchRejected;
        assert!(wrong_classification.validate(&step).is_err());

        let mut wrong_step = failure.clone();
        wrong_step.step_id = "other".into();
        assert!(wrong_step.validate(&step).is_err());

        let mut invalid_message = failure.clone();
        invalid_message.message = "provider\nbody".into();
        assert!(invalid_message.validate(&step).is_err());

        let execution = routed_execution_workflow_run_input()
            .expect("routed Execution WorkflowRun input")
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == TEST_EXECUTION_STEP_ID)
            .expect("Execution step");
        assert!(failure.validate(&execution).is_err());
    }
}
