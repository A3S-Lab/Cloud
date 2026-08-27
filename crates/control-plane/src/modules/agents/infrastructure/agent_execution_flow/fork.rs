use super::{flow_error, AgentExecutionFlowRuntime};
use crate::modules::agents::domain::{
    AgentExecution, AgentExecutionCheckpointEvent, AgentExecutionCheckpointSnapshot,
};
use crate::modules::shared_kernel::domain::{AgentExecutionCheckpointId, AgentExecutionId};
use a3s_cloud_contracts::AGENT_PROVIDER_MAX_PROMPT_BYTES;
use a3s_flow::FlowError;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentExecutionForkPrompt<'a> {
    schema: &'static str,
    parent_execution_id: AgentExecutionId,
    parent_checkpoint_id: AgentExecutionCheckpointId,
    parent_checkpoint_digest: &'a str,
    through_event_sequence: u64,
    trajectory: &'a [AgentExecutionCheckpointEvent],
    input: &'a serde_json::Value,
}

pub(super) async fn execution_prompt(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
    input: &serde_json::Value,
) -> a3s_flow::Result<String> {
    let Some(lineage) = execution.lineage.as_ref() else {
        return match input {
            serde_json::Value::String(prompt) => Ok(prompt.clone()),
            input => serde_json::to_string(input).map_err(FlowError::from),
        };
    };
    let checkpoint = runtime
        .agents
        .find_execution_checkpoint(execution.organization_id, lineage.parent_checkpoint_id)
        .await
        .map_err(|error| flow_error("could not load Agent fork checkpoint", error))?
        .ok_or_else(|| FlowError::Runtime("Agent fork checkpoint no longer exists".into()))?;
    if checkpoint.organization_id != execution.organization_id
        || checkpoint.conversation_id != execution.conversation_id
        || checkpoint.execution_id != lineage.parent_execution_id
        || checkpoint.id != lineage.parent_checkpoint_id
        || checkpoint.object.digest != lineage.parent_checkpoint_digest
        || &checkpoint.agent_artifact_digest != execution.agent.artifact_digest()
        || checkpoint.provider_profile_digest.as_str() != execution.provider.profile_digest()
    {
        return Err(FlowError::Runtime(
            "Agent fork checkpoint changed its immutable lineage".into(),
        ));
    }
    let bytes = runtime
        .checkpoint_objects
        .get(&checkpoint.object)
        .await
        .map_err(|error| flow_error("could not read Agent fork checkpoint object", error))?;
    let snapshot = serde_json::from_slice::<AgentExecutionCheckpointSnapshot>(&bytes)
        .map_err(|error| flow_error("could not decode Agent fork checkpoint object", error))?;
    checkpoint
        .validate_snapshot(&snapshot)
        .map_err(|error| flow_error("could not verify Agent fork checkpoint object", error))?;
    let invocation_digest = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent execution has no provider binding".into()))?
        .require_invocation_profile()
        .map_err(|error| flow_error("could not restore Harness invocation profile", error))?
        .digest()
        .map_err(|error| flow_error("could not digest Harness invocation profile", error))?;
    if checkpoint.invocation_profile_digest.as_str() != invocation_digest {
        return Err(FlowError::Runtime(
            "Agent fork Runtime changed its checkpoint invocation profile".into(),
        ));
    }
    let prompt = AgentExecutionForkPrompt {
        schema: "a3s.cloud.agent-execution-fork-prompt.v1",
        parent_execution_id: lineage.parent_execution_id,
        parent_checkpoint_id: lineage.parent_checkpoint_id,
        parent_checkpoint_digest: lineage.parent_checkpoint_digest.as_str(),
        through_event_sequence: checkpoint.through_event_sequence,
        trajectory: &snapshot.events,
        input,
    };
    let encoded = serde_json::to_vec(&prompt)?;
    if encoded.len() > AGENT_PROVIDER_MAX_PROMPT_BYTES {
        return Err(FlowError::Runtime(format!(
            "Agent fork prompt exceeds the {AGENT_PROVIDER_MAX_PROMPT_BYTES}-byte provider limit"
        )));
    }
    String::from_utf8(encoded)
        .map_err(|error| flow_error("could not encode Agent fork prompt as UTF-8", error))
}
