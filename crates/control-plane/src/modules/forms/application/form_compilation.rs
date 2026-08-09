use crate::modules::forms::domain::{FormDocument, FormReleaseContent, IFormSemanticCore};
use crate::modules::shared_kernel::application::ApplicationError;
use a3s_form_core::{
    canonicalize_json, parse_json, CompileOptions, CompileRequest, COMPILE_REQUEST_API_VERSION,
    COMPILE_RESPONSE_API_VERSION,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompileResponseEnvelope {
    api_version: String,
    compiler_revision: String,
    ok: bool,
    normalized_document_json: Option<String>,
    digest: Option<String>,
    schema_profile: Option<String>,
    form_plan: Option<serde_json::Value>,
    diagnostics: Vec<serde_json::Value>,
}

pub(super) async fn compile_release_content(
    semantic_core: Arc<dyn IFormSemanticCore>,
    document: &FormDocument,
) -> Result<FormReleaseContent, ApplicationError> {
    let document = parse_json(document.canonical_json().as_bytes()).map_err(|error| {
        ApplicationError::Internal(format!("stored Form draft could not be decoded: {error}"))
    })?;
    let request = CompileRequest {
        api_version: COMPILE_REQUEST_API_VERSION.into(),
        document,
        options: CompileOptions::default(),
    };
    let request = serde_json::to_vec(&request).map_err(|error| {
        ApplicationError::Internal(format!("Form compile request failed: {error}"))
    })?;
    let request = canonicalize_json(&request).map_err(|error| {
        ApplicationError::Internal(format!("Form compile request is not canonical: {error}"))
    })?;
    let expected_compiler_revision = semantic_core.compiler_revision();
    let response = tokio::task::spawn_blocking(move || semantic_core.compile(&request))
        .await
        .map_err(|error| ApplicationError::Internal(format!("Form compiler task failed: {error}")))?
        .map_err(|error| ApplicationError::Unavailable(error.to_string()))?;
    let response: CompileResponseEnvelope = serde_json::from_slice(&response).map_err(|error| {
        ApplicationError::Internal(format!("Form compiler response is invalid JSON: {error}"))
    })?;
    if response.api_version != COMPILE_RESPONSE_API_VERSION
        || response.compiler_revision != expected_compiler_revision
    {
        return Err(ApplicationError::Internal(
            "Form compiler returned an incompatible protocol identity".into(),
        ));
    }
    if !response.ok {
        return Err(ApplicationError::Invalid(compilation_failure(
            &response.diagnostics,
        )));
    }
    let normalized_document_json = response.normalized_document_json.ok_or_else(|| {
        ApplicationError::Internal("successful Form compilation omitted the document".into())
    })?;
    let digest = response.digest.ok_or_else(|| {
        ApplicationError::Internal("successful Form compilation omitted the digest".into())
    })?;
    let schema_profile = response.schema_profile.ok_or_else(|| {
        ApplicationError::Internal("successful Form compilation omitted the schema profile".into())
    })?;
    let plan = response.form_plan.ok_or_else(|| {
        ApplicationError::Internal("successful Form compilation omitted the Form plan".into())
    })?;
    let plan = serde_json::to_vec(&plan).map_err(|error| {
        ApplicationError::Internal(format!("Form plan could not be encoded: {error}"))
    })?;
    let plan = canonicalize_json(&plan).map_err(|error| {
        ApplicationError::Internal(format!("Form plan could not be canonicalized: {error}"))
    })?;
    let form_plan_json = String::from_utf8(plan)
        .map_err(|_| ApplicationError::Internal("Form plan is not UTF-8".into()))?;
    FormReleaseContent::restore(
        normalized_document_json,
        form_plan_json,
        response.compiler_revision,
        schema_profile,
        &digest,
    )
    .map_err(|error| {
        ApplicationError::Internal(format!("Form compiler response is inconsistent: {error}"))
    })
}

fn compilation_failure(diagnostics: &[serde_json::Value]) -> String {
    let Some(diagnostic) = diagnostics.first() else {
        return "Form compilation failed without a diagnostic".into();
    };
    let code = diagnostic
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("compile.failed");
    let message = diagnostic
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Form compilation failed");
    let path = diagnostic
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    format!("Form compilation failed ({code}) at {path}: {message}")
}
