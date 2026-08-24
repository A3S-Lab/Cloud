use super::{WorkflowDefaultOutput, WorkflowRetryPolicy, WORKFLOW_DEFAULT_OUTPUT_MAX_BYTES};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, Sha256Digest};
use a3s_acl::Block;
use serde_json::Value;

pub(super) fn required_label(block: &Block, label: &str) -> Result<String, String> {
    block
        .labels
        .first()
        .cloned()
        .ok_or_else(|| format!("{label} label is missing"))
}

pub(super) fn required_string(block: &Block, name: &str) -> Result<String, String> {
    optional_string(block, name)?.ok_or_else(|| format!("{name} is missing"))
}

pub(super) fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{name} must be a string"))
        })
        .transpose()
}

pub(super) fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    block
        .attributes
        .get(name)
        .and_then(|value| value.as_bool())
        .ok_or_else(|| format!("{name} must be a boolean"))
}

pub(super) fn required_number(block: &Block, name: &str) -> Result<f64, String> {
    optional_number(block, name)?.ok_or_else(|| format!("{name} is missing"))
}

pub(super) fn optional_number(block: &Block, name: &str) -> Result<Option<f64>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_number()
                .ok_or_else(|| format!("{name} must be a number"))
        })
        .transpose()
}

pub(super) fn positive_integer(value: f64) -> Result<u64, String> {
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 || value > u64::MAX as f64 {
        return Err("Workflow payload number must be a positive integer".into());
    }
    Ok(value as u64)
}

pub(super) fn non_negative_integer(value: f64) -> Result<u64, String> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > u64::MAX as f64 {
        return Err("Workflow payload number must be a non-negative integer".into());
    }
    Ok(value as u64)
}

pub(super) fn parse_retry_policy(block: &Block) -> Result<WorkflowRetryPolicy, String> {
    Ok(WorkflowRetryPolicy {
        maximum_attempts: u32::try_from(positive_integer(required_number(
            block,
            "maximum_attempts",
        )?)?)
        .map_err(|_| "Workflow retry maximum_attempts exceeds u32".to_owned())?,
        default_delay_seconds: positive_integer(required_number(block, "default_delay_seconds")?)?,
    })
}

pub(super) fn parse_default_output(block: &Block) -> Result<WorkflowDefaultOutput, String> {
    let canonical_json = required_string(block, "canonical_json")?;
    let value = serde_json::from_str::<Value>(&canonical_json)
        .map_err(|error| format!("Workflow default-output JSON is invalid: {error}"))?;
    let output = WorkflowDefaultOutput {
        port: required_label(block, "Workflow default-output port")?,
        value,
        digest: Sha256Digest::parse(required_string(block, "digest")?)?,
    };
    output.validate()?;
    let encoded = String::from_utf8(canonical_default_output_bytes(&output.value)?)
        .map_err(|_| "Workflow default-output JSON is not UTF-8".to_owned())?;
    if encoded != canonical_json {
        return Err("Workflow default-output JSON is not canonical".into());
    }
    Ok(output)
}

pub(super) fn canonical_default_output_bytes(value: &Value) -> Result<Vec<u8>, String> {
    canonical_json_bounded(
        value,
        WORKFLOW_DEFAULT_OUTPUT_MAX_BYTES,
        "Workflow default output",
    )
}
