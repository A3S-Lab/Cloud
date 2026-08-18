mod attestation;
mod box_execution;
mod cleanup;
mod common;
mod prepare;
mod publication;
mod validation;

use super::BuildFlowRuntime;
use a3s_flow::{FlowError, StepInvocation};

pub(super) const BUILD_ATTEST_OUTPUT: &str = "build_attest_output";
pub(super) const BUILD_PREPARE_INPUT: &str = "build_prepare_input";
pub(super) const BUILD_SCHEDULE_BOX: &str = "build_schedule_box";
pub(super) const BUILD_DISPATCH_BOX: &str = "build_dispatch_box";
pub(super) const BUILD_OBSERVE_BOX: &str = "build_observe_box";
pub(super) const BUILD_VALIDATE_OUTPUT: &str = "build_validate_output";
pub(super) const BUILD_PREPARE_PUBLICATION: &str = "build_prepare_publication";
pub(super) const BUILD_PUBLISH_OUTPUT: &str = "build_publish_output";
pub(super) const BUILD_FAIL: &str = "build_fail";
pub(super) const BUILD_CLEANUP_DISPATCH: &str = "build_cleanup_dispatch";
pub(super) const BUILD_CLEANUP_OBSERVE: &str = "build_cleanup_observe";
pub(super) const BUILD_COMPLETE: &str = "build_complete";

pub(super) const STEP_NAMES: &[&str] = &[
    BUILD_ATTEST_OUTPUT,
    BUILD_PREPARE_INPUT,
    BUILD_SCHEDULE_BOX,
    BUILD_DISPATCH_BOX,
    BUILD_OBSERVE_BOX,
    BUILD_VALIDATE_OUTPUT,
    BUILD_PREPARE_PUBLICATION,
    BUILD_PUBLISH_OUTPUT,
    BUILD_FAIL,
    BUILD_CLEANUP_DISPATCH,
    BUILD_CLEANUP_OBSERVE,
    BUILD_COMPLETE,
];

pub(super) async fn execute(
    runtime: &BuildFlowRuntime,
    invocation: StepInvocation,
) -> a3s_flow::Result<serde_json::Value> {
    let run_id = invocation.run_id.clone();
    match invocation.step_name.as_str() {
        BUILD_ATTEST_OUTPUT => {
            encode(attestation::attest(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_PREPARE_INPUT => {
            encode(prepare::prepare(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_SCHEDULE_BOX => {
            encode(box_execution::schedule(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_DISPATCH_BOX => {
            encode(box_execution::dispatch(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_OBSERVE_BOX => {
            encode(box_execution::observe(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_VALIDATE_OUTPUT => {
            encode(validation::validate(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_PREPARE_PUBLICATION => {
            encode(publication::prepare(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_PUBLISH_OUTPUT => {
            encode(publication::publish(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_FAIL => encode(validation::fail(runtime, &run_id, invocation.input_as()?).await?),
        BUILD_CLEANUP_DISPATCH => {
            encode(cleanup::dispatch(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_CLEANUP_OBSERVE => {
            encode(cleanup::observe(runtime, &run_id, invocation.input_as()?).await?)
        }
        BUILD_COMPLETE => {
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
