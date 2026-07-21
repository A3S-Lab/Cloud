mod cleanup;
mod common;
mod prepare;
mod publication;
mod runtime;
mod validation;

use super::BuildFlowRuntime;
use a3s_flow::{FlowError, StepInvocation};

pub(super) async fn execute(
    runtime: &BuildFlowRuntime,
    invocation: StepInvocation,
) -> a3s_flow::Result<serde_json::Value> {
    let run_id = invocation.run_id.clone();
    match invocation.step_name.as_str() {
        "build_prepare_input" => {
            encode(prepare::prepare(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_schedule_runtime" => {
            encode(runtime::schedule(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_dispatch_runtime" => {
            encode(runtime::dispatch(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_observe_runtime" => {
            encode(runtime::observe(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_validate_output" => {
            encode(validation::validate(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_prepare_publication" => {
            encode(publication::prepare(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_publish_output" => {
            encode(publication::publish(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_fail" => encode(validation::fail(runtime, &run_id, invocation.input_as()?).await?),
        "build_cleanup_dispatch" => {
            encode(cleanup::dispatch(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_cleanup_observe" => {
            encode(cleanup::observe(runtime, &run_id, invocation.input_as()?).await?)
        }
        "build_complete" => {
            encode(validation::complete(runtime, &run_id, invocation.input_as()?).await?)
        }
        _ => Err(FlowError::Runtime(format!(
            "Cloud has no build step runtime for {}",
            invocation.step_name
        ))),
    }
}

fn encode<T: serde::Serialize>(value: T) -> a3s_flow::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(FlowError::from)
}
