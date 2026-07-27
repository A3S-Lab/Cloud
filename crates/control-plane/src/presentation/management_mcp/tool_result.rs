use crate::modules::shared_kernel::application::ApplicationError;
use crate::presentation::{api_success_envelope, application_error_envelope};
use a3s_boot::{BootError, Result};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn success<T>(status: u16, data: T, request_id: Uuid) -> Result<Value>
where
    T: Serialize,
{
    let envelope = api_success_envelope(status, data, request_id);
    let structured =
        serde_json::to_value(&envelope).map_err(|error| BootError::Internal(error.to_string()))?;
    let text =
        serde_json::to_string(&envelope).map_err(|error| BootError::Internal(error.to_string()))?;
    Ok(json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": false
    }))
}

pub fn application_error(error: ApplicationError, request_id: Uuid) -> Result<Value> {
    let envelope = application_error_envelope(error, request_id);
    let structured =
        serde_json::to_value(&envelope).map_err(|error| BootError::Internal(error.to_string()))?;
    let text =
        serde_json::to_string(&envelope).map_err(|error| BootError::Internal(error.to_string()))?;
    Ok(json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": true
    }))
}
