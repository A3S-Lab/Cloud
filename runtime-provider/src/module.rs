use std::sync::Arc;

use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, Module, ModuleRef,
    ProviderDefinition, Result,
};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeExecRequest, RuntimeLogQuery,
};
use a3s_runtime::{ManagedRuntimeClient, RuntimeClient, RuntimeError};
use serde::Deserialize;

use crate::driver::ProcessRuntimeDriver;

#[derive(Clone)]
pub struct RuntimeProviderModule {
    client: Arc<ManagedRuntimeClient>,
    driver: Arc<ProcessRuntimeDriver>,
    api_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectRequest {
    unit_id: String,
}

impl RuntimeProviderModule {
    pub fn new(
        client: Arc<ManagedRuntimeClient>,
        driver: Arc<ProcessRuntimeDriver>,
        api_token: String,
    ) -> Self {
        Self {
            client,
            driver,
            api_token,
        }
    }

    fn lifecycle_controller(
        client: Arc<ManagedRuntimeClient>,
        token: String,
    ) -> Result<ControllerDefinition> {
        let capabilities_client = Arc::clone(&client);
        let capabilities_token = token.clone();
        let apply_client = Arc::clone(&client);
        let apply_token = token.clone();
        let inspect_client = Arc::clone(&client);
        let inspect_token = token.clone();
        let stop_client = Arc::clone(&client);
        let stop_token = token.clone();
        let remove_client = Arc::clone(&client);
        let remove_token = token.clone();
        let logs_client = Arc::clone(&client);
        let logs_token = token.clone();

        ControllerDefinition::new("/v1")?
            .get("/capabilities", move |request: BootRequest| {
                let client = Arc::clone(&capabilities_client);
                let token = capabilities_token.clone();
                async move {
                    authorize(&request, &token)?;
                    let value = client.capabilities().await.map_err(map_runtime_error)?;
                    BootResponse::json(&value)
                }
            })?
            .post("/units/apply", move |request: BootRequest| {
                let client = Arc::clone(&apply_client);
                let token = apply_token.clone();
                async move {
                    authorize(&request, &token)?;
                    let payload = request.json::<RuntimeApplyRequest>()?;
                    let value = client.apply(&payload).await.map_err(map_runtime_error)?;
                    BootResponse::json(&value)
                }
            })?
            .post("/units/inspect", move |request: BootRequest| {
                let client = Arc::clone(&inspect_client);
                let token = inspect_token.clone();
                async move {
                    authorize(&request, &token)?;
                    let payload = request.json::<InspectRequest>()?;
                    let value = client
                        .inspect(&payload.unit_id)
                        .await
                        .map_err(map_runtime_error)?;
                    BootResponse::json(&value)
                }
            })?
            .post("/units/stop", move |request: BootRequest| {
                let client = Arc::clone(&stop_client);
                let token = stop_token.clone();
                async move {
                    authorize(&request, &token)?;
                    let payload = request.json::<RuntimeActionRequest>()?;
                    let value = client.stop(&payload).await.map_err(map_runtime_error)?;
                    BootResponse::json(&value)
                }
            })?
            .post("/units/remove", move |request: BootRequest| {
                let client = Arc::clone(&remove_client);
                let token = remove_token.clone();
                async move {
                    authorize(&request, &token)?;
                    let payload = request.json::<RuntimeActionRequest>()?;
                    let value = client.remove(&payload).await.map_err(map_runtime_error)?;
                    BootResponse::json(&value)
                }
            })?
            .post("/units/logs", move |request: BootRequest| {
                let client = Arc::clone(&logs_client);
                let token = logs_token.clone();
                async move {
                    authorize(&request, &token)?;
                    let payload = request.json::<RuntimeLogQuery>()?;
                    let value = client.logs(&payload).await.map_err(map_runtime_error)?;
                    BootResponse::json(&value)
                }
            })?
            .post("/units/exec", move |request: BootRequest| {
                let client = Arc::clone(&client);
                let token = token.clone();
                async move {
                    authorize(&request, &token)?;
                    let payload = request.json::<RuntimeExecRequest>()?;
                    let value = client.exec(&payload).await.map_err(map_runtime_error)?;
                    BootResponse::json(&value)
                }
            })
    }

    fn artifact_controller(driver: Arc<ProcessRuntimeDriver>) -> Result<ControllerDefinition> {
        ControllerDefinition::new("/v1/artifacts")?.get("/{digest}", move |request: BootRequest| {
            let driver = Arc::clone(&driver);
            async move {
                let digest = request
                    .param("digest")
                    .ok_or_else(|| BootError::BadRequest("missing digest".to_string()))?;
                if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(BootError::BadRequest(
                        "artifact digest must be 64 hexadecimal characters".to_string(),
                    ));
                }
                let bytes = tokio::fs::read(driver.artifact_path(digest))
                    .await
                    .map_err(|error| match error.kind() {
                        std::io::ErrorKind::NotFound => {
                            BootError::NotFound("artifact not found".to_string())
                        }
                        _ => BootError::Internal(error.to_string()),
                    })?;
                Ok(BootResponse::new(200, bytes)
                    .with_header("content-type", "application/octet-stream")
                    .with_header("cache-control", "public, immutable, max-age=31536000"))
            }
        })
    }
}

impl Module for RuntimeProviderModule {
    fn name(&self) -> &'static str {
        "runtime-provider"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(Vec::new())
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            Self::lifecycle_controller(Arc::clone(&self.client), self.api_token.clone())?,
            Self::artifact_controller(Arc::clone(&self.driver))?,
        ])
    }
}

fn authorize(request: &BootRequest, token: &str) -> Result<()> {
    if token.is_empty() {
        return Ok(());
    }
    let expected = format!("Bearer {token}");
    if request.header("authorization") == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(BootError::Unauthorized(
            "invalid Runtime provider token".to_string(),
        ))
    }
}

fn map_runtime_error(error: RuntimeError) -> BootError {
    match error {
        RuntimeError::InvalidRequest(message) => BootError::BadRequest(message),
        RuntimeError::NotFound { unit_id } => BootError::NotFound(unit_id),
        RuntimeError::RequestConflict { request_id }
        | RuntimeError::RequestNotFound { request_id, .. } => BootError::Conflict(request_id),
        RuntimeError::StaleGeneration { .. } | RuntimeError::GenerationConflict { .. } => {
            BootError::Conflict(error.to_string())
        }
        RuntimeError::DeadlineExceeded(message) => BootError::RequestTimeout(message),
        RuntimeError::UnsupportedCapabilities(missing) => {
            BootError::UnprocessableEntity(format!("missing Runtime capabilities: {missing:?}"))
        }
        RuntimeError::ProviderUnavailable(message) | RuntimeError::Transport(message) => {
            BootError::ServiceUnavailable(message)
        }
        RuntimeError::Protocol(message) => BootError::BadGateway(message),
    }
}
