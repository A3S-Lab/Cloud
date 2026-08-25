use super::{execution, WorkflowLocalStepResult};
use crate::modules::connectors::domain::ConnectorResponseObjectReference;
use crate::modules::connectors::{IConnectorResponseObjectPort, ReadConnectorResponseObject};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::domain::canonical_json_bounded;
use crate::modules::workflow::domain::{
    flow_step_id, CapabilityType, ResolvedWorkflowRunStep, WorkflowConnectorAttemptEvidence,
    WorkflowConnectorAttemptOutcome, WorkflowConnectorHookMetadata, WorkflowConnectorStepOutput,
    WorkflowStepKind, WORKFLOW_RUN_INPUT_MAX_BYTES, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9,
};
use serde::de::{Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use std::fmt;

pub(super) const WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME: &str = "workflow_connector_response";
const WORKFLOW_CONNECTOR_RESPONSE_STEP_SCHEMA: &str = "cloud.workflow.connector-response-step.v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WorkflowConnectorResponseStepInput {
    schema: String,
    runtime_contract_revision: String,
    step: ResolvedWorkflowRunStep,
    metadata: WorkflowConnectorHookMetadata,
    evidence: WorkflowConnectorAttemptEvidence,
}

impl WorkflowConnectorResponseStepInput {
    pub(super) fn new(
        runtime_contract_revision: &str,
        step: &ResolvedWorkflowRunStep,
        metadata: &WorkflowConnectorHookMetadata,
        evidence: &WorkflowConnectorAttemptEvidence,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_CONNECTOR_RESPONSE_STEP_SCHEMA.into(),
            runtime_contract_revision: runtime_contract_revision.into(),
            step: step.clone(),
            metadata: metadata.clone(),
            evidence: evidence.clone(),
        };
        value.validate()?;
        Ok(value)
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_CONNECTOR_RESPONSE_STEP_SCHEMA
            || !matches!(
                self.runtime_contract_revision.as_str(),
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
            )
            || self.step.plan.kind != WorkflowStepKind::Service
            || self.step.plan.id != self.metadata.step_id
            || self.step.plan.configuration_digest != self.metadata.configuration_digest
            || !self.metadata.requires_typed_response()
            || self.evidence.outcome != WorkflowConnectorAttemptOutcome::Accepted
        {
            return Err("Workflow Connector response-step authority is invalid".into());
        }
        self.metadata.validate()?;
        let capability = self
            .step
            .plan
            .capability
            .as_ref()
            .ok_or_else(|| "Workflow Connector response step lost its capability".to_owned())?;
        capability.validate()?;
        if capability.capability_type != CapabilityType::ConnectorRevision
            || capability.resource_id != self.metadata.connector_profile_id.as_uuid()
            || capability.revision != self.metadata.connector_revision_id.to_string()
            || capability.digest != self.metadata.connector_revision_digest
            || capability.capability != self.metadata.capability
            || self.step.plan.policy_digest.as_ref() != Some(&self.metadata.retry_policy_digest)
            || self.step.policy.as_ref().and_then(|policy| policy.retry)
                != Some(self.metadata.retry_policy)
        {
            return Err("Workflow Connector response-step binding drifted".into());
        }
        let authority = super::connector::attempt_authority(&self.metadata)?;
        WorkflowConnectorStepOutput::from_evidence(
            &self.metadata,
            &self.evidence,
            authority.attempt_id,
            &authority.request_digest,
            authority.request_body_bytes,
        )?;
        self.connector_reference()?;
        canonical_json_bounded(
            self,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow Connector response-step input",
        )?;
        Ok(())
    }

    pub(super) fn validate_result(&self, result: &WorkflowLocalStepResult) -> Result<(), String> {
        self.validate()?;
        result.validate(&self.step)
    }

    fn connector_reference(&self) -> Result<ConnectorResponseObjectReference, String> {
        let reference = self
            .evidence
            .response_object
            .as_ref()
            .ok_or_else(|| "accepted Workflow Connector response lost its object".to_owned())?;
        reference.validate(self.evidence.connector_attempt_id)?;
        let connector_reference = ConnectorResponseObjectReference::new(
            self.metadata.organization_id,
            self.metadata.project_id,
            self.metadata.environment_id,
            self.metadata.connector_profile_id,
            self.metadata.connector_revision_id,
            reference.connector_attempt_id,
            reference.digest.clone(),
            reference.size_bytes,
        )?;
        if connector_reference.object_ref != reference.object_ref {
            return Err("Workflow Connector response object path drifted".into());
        }
        Ok(connector_reference)
    }
}

impl fmt::Debug for WorkflowConnectorResponseStepInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowConnectorResponseStepInput")
            .field("workflow_run_id", &self.metadata.workflow_run_id)
            .field("step_id", &self.metadata.step_id)
            .field("step_attempt", &self.metadata.step_attempt)
            .field("connector_attempt_id", &self.evidence.connector_attempt_id)
            .field("response_body_bytes", &self.evidence.response_body_bytes)
            .finish_non_exhaustive()
    }
}

pub(super) async fn consume_response(
    input: &WorkflowConnectorResponseStepInput,
    responses: &dyn IConnectorResponseObjectPort,
) -> Result<WorkflowLocalStepResult, String> {
    input.validate()?;
    let reference = input.connector_reference()?;
    let request = ReadConnectorResponseObject {
        reference: reference.clone(),
        resource_access: ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id: reference.project_id,
            environment_id: reference.environment_id,
        }]),
    };
    let content = responses
        .read_response_object(&request)
        .await
        .map_err(|_| "Workflow Connector response object could not be read".to_owned())?;
    if content.reference() != &reference {
        return Err("Workflow Connector response reader changed its authority".into());
    }
    let output = parse_json_output(content.body(), &input.step.output_schema)?;
    let result = WorkflowLocalStepResult {
        step_id: input.step.plan.id.clone(),
        kind: WorkflowStepKind::Service,
        output_digest: execution::value_digest(&output, "Workflow Connector typed output")?,
        output,
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    input.validate_result(&result)?;
    Ok(result)
}

pub(super) fn verify_step_history(
    input: &crate::modules::workflow::domain::WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    observed: &[super::connector::ObservedConnectorHook<'_>],
    snapshot: &a3s_flow::WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<(), String> {
    let expected = observed
        .last()
        .map(|hook| super::connector::accepted_response_step_input(input, step, hook))
        .transpose()?
        .flatten();
    let durable_step_id = flow_step_id(&step.plan.id);
    let flow_step = snapshot.steps.get(&durable_step_id);
    let Some(expected) = expected else {
        if flow_step.is_some() {
            return Err("Workflow Connector history contains an unauthorized response step".into());
        }
        return Ok(());
    };
    let Some(flow_step) = flow_step else {
        if snapshot.status.is_terminal() {
            return Err("terminal Workflow Connector response lost its typed step".into());
        }
        return Ok(());
    };
    let created = history
        .iter()
        .filter(|event| {
            matches!(
                &event.event,
                a3s_flow::FlowEvent::StepCreated { step_id, .. }
                    if step_id == &durable_step_id
            )
        })
        .collect::<Vec<_>>();
    if created.len() != 1 {
        return Err("Workflow Connector response step must have one creation event".into());
    }
    let a3s_flow::FlowEvent::StepCreated {
        step_name,
        input: observed_input,
        retry,
        ..
    } = &created[0].event
    else {
        return Err("Workflow Connector response step creation history is invalid".into());
    };
    let expected_input = serde_json::to_value(&expected)
        .map_err(|error| format!("could not encode Workflow Connector response step: {error}"))?;
    let expected_retry = if super::workflow::failure_route_handle(input, step)
        .map_err(|_| "Workflow Connector failure route is invalid".to_owned())?
        .is_some()
    {
        a3s_flow::RetryPolicy::none().continue_workflow_on_failure()
    } else {
        a3s_flow::RetryPolicy::none()
    };
    if step_name != WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME
        || observed_input != &expected_input
        || retry != &expected_retry
    {
        return Err("Workflow Connector response step creation authority drifted".into());
    }
    if let Some(output) = flow_step.output.as_ref() {
        let result = serde_json::from_value::<WorkflowLocalStepResult>(output.clone())
            .map_err(|error| format!("Workflow Connector response result is invalid: {error}"))?;
        expected.validate_result(&result)?;
    }
    Ok(())
}

pub(super) fn parse_json_output(
    body: &[u8],
    schema: &crate::modules::workflow::domain::WorkflowDataSchema,
) -> Result<Value, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let value = StrictJsonValue::deserialize(&mut deserializer)
        .and_then(|value| deserializer.end().map(|_| value.0))
        .map_err(|_| {
            "Workflow Connector response body is not one unambiguous JSON value".to_owned()
        })?;
    execution::validate_data_schema(schema, &value, "Workflow Connector typed output").map_err(
        |_| {
            "Workflow Connector JSON response does not match its immutable output schema".to_owned()
        },
    )?;
    canonical_json_bounded(
        &value,
        WORKFLOW_RUN_OUTPUT_MAX_BYTES,
        "Workflow Connector typed output",
    )
    .map_err(|_| "Workflow Connector JSON response exceeds the output bound".to_owned())?;
    Ok(value)
}

struct StrictJsonValue(Value);

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("one JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(StrictJsonValue)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value.into())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictJsonValue>()? {
            values.push(value.0);
        }
        Ok(StrictJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, value)) = object.next_entry::<String, StrictJsonValue>()? {
            if values.insert(key, value.0).is_some() {
                return Err(A::Error::custom("duplicate JSON object key"));
            }
        }
        Ok(StrictJsonValue(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use crate::modules::workflow::domain::{
        WorkflowConnectorResponseObjectReference, WORKFLOW_RUN_INPUT_SCHEMA_V13,
    };
    use crate::modules::workflow::test_support::{connector_workflow_run_input, timestamp};

    #[test]
    fn v13_connector_response_step_retains_typed_object_authority() {
        let mut input = connector_workflow_run_input().expect("Connector WorkflowRun input");
        input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V13.into();
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == "invoke")
            .expect("Connector step");
        let metadata = WorkflowConnectorHookMetadata::from_run_step(
            &input,
            &step,
            input.goal_input.clone(),
            1,
            1,
        )
        .expect("Connector metadata");
        let authority = super::super::connector::attempt_authority(&metadata)
            .expect("Connector attempt authority");
        let body = br#"{"accepted":true}"#;
        let response_digest = Sha256Digest::from_bytes(body);
        let hexadecimal = response_digest
            .as_str()
            .strip_prefix("sha256:")
            .expect("response digest");
        let response_object = WorkflowConnectorResponseObjectReference::new(
            authority.attempt_id,
            format!(
                "attempts/{}/sha256/{hexadecimal}/body",
                authority.attempt_id
            ),
            response_digest.clone(),
            body.len() as u64,
        )
        .expect("response object");
        let evidence = WorkflowConnectorAttemptEvidence::restore_with_response_object(
            authority.attempt_id,
            authority.request_digest,
            authority.request_body_bytes,
            WorkflowConnectorAttemptOutcome::Accepted,
            Some(200),
            Some(response_digest),
            Some(body.len() as u64),
            Some(response_object),
            None,
            timestamp(8, 1),
            timestamp(8, 2),
        )
        .expect("accepted Connector evidence");

        WorkflowConnectorResponseStepInput::new(
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13,
            &step,
            &metadata,
            &evidence,
        )
        .expect("v13 typed response step")
        .validate()
        .expect("valid v13 response authority");
    }
}
