use crate::modules::forms::domain::{FormRelease, IFormRepository, IFormSemanticCore};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{FormId, FormReleaseId};
use crate::modules::workflow::application::{
    HumanTaskFormEvaluation, HumanTaskFormReleaseAuthority, IHumanTaskFormPort,
};
use a3s_form_core::{
    canonicalize_json, parse_json, CanonicalValue, EvaluateRequest, EvaluationOptions,
    FormReleaseMode, FormReleaseRef, EVALUATE_REQUEST_API_VERSION, EVALUATE_RESPONSE_API_VERSION,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvaluationResponseEnvelope {
    api_version: String,
    compiler_revision: String,
    ok: bool,
    value: Option<CanonicalValue>,
    #[serde(rename = "trace")]
    _trace: Vec<serde_json::Value>,
    errors: Vec<EvaluationErrorEnvelope>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluationErrorEnvelope {
    path: String,
    code: String,
    message: String,
}

pub struct FormsHumanTaskFormAdapter {
    forms: Arc<dyn IFormRepository>,
    semantic_core: Arc<dyn IFormSemanticCore>,
}

impl FormsHumanTaskFormAdapter {
    pub fn new(forms: Arc<dyn IFormRepository>, semantic_core: Arc<dyn IFormSemanticCore>) -> Self {
        Self {
            forms,
            semantic_core,
        }
    }

    async fn find_release(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        form_id: FormId,
        release_id: FormReleaseId,
    ) -> ApplicationResult<FormRelease> {
        self.forms
            .find_release(organization_id, form_id, release_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| {
                ApplicationError::Conflict("HumanTask FormRelease does not exist".into())
            })
    }
}

#[async_trait]
impl IHumanTaskFormPort for FormsHumanTaskFormAdapter {
    async fn resolve_interaction_release(
        &self,
        authority: &HumanTaskFormReleaseAuthority,
    ) -> ApplicationResult<FormReleaseRef> {
        authority.validate().map_err(ApplicationError::Internal)?;
        let release = self
            .find_release(
                authority.organization_id,
                authority.form_id,
                authority.form_release_id,
            )
            .await?;
        release.validate().map_err(ApplicationError::Internal)?;
        let release_ref = release.release_ref().map_err(ApplicationError::Internal)?;
        if release.organization_id != authority.organization_id
            || release.project_id != authority.project_id
            || release.form_id != authority.form_id
            || release.id != authority.form_release_id
            || release.content.digest() != &authority.form_release_digest
            || release_ref.mode != FormReleaseMode::Interaction
        {
            return Err(ApplicationError::Conflict(
                "HumanTask FormRelease authority drifted".into(),
            ));
        }
        Ok(release_ref)
    }

    async fn evaluate_submission(
        &self,
        request: &HumanTaskFormEvaluation,
    ) -> ApplicationResult<CanonicalValue> {
        request.validate().map_err(ApplicationError::Internal)?;
        let form_id = Uuid::parse_str(&request.form_release.form_id)
            .map(FormId::from_uuid)
            .map_err(|_| ApplicationError::Internal("HumanTask Form identity is invalid".into()))?;
        let release_id = Uuid::parse_str(&request.form_release.release_id)
            .map(FormReleaseId::from_uuid)
            .map_err(|_| {
                ApplicationError::Internal("HumanTask Form release identity is invalid".into())
            })?;
        let release = self
            .find_release(request.organization_id, form_id, release_id)
            .await?;
        if release.project_id != request.project_id
            || release.release_ref().map_err(ApplicationError::Internal)? != request.form_release
        {
            return Err(ApplicationError::Conflict(
                "HumanTask Form release authority drifted".into(),
            ));
        }
        evaluate_submission(
            Arc::clone(&self.semantic_core),
            release.content.form_plan_json(),
            &request.candidate,
        )
        .await
    }
}

async fn evaluate_submission(
    semantic_core: Arc<dyn IFormSemanticCore>,
    form_plan_json: &str,
    candidate: &CanonicalValue,
) -> ApplicationResult<CanonicalValue> {
    let form_plan = parse_json(form_plan_json.as_bytes()).map_err(|error| {
        ApplicationError::Internal(format!("stored Form plan could not be decoded: {error}"))
    })?;
    let request = EvaluateRequest {
        api_version: EVALUATE_REQUEST_API_VERSION.into(),
        form_plan,
        value: candidate.clone(),
        options: EvaluationOptions::default(),
    };
    let request = serde_json::to_vec(&request).map_err(|error| {
        ApplicationError::Internal(format!("Form evaluation request failed: {error}"))
    })?;
    let request = canonicalize_json(&request).map_err(|error| {
        ApplicationError::Internal(format!("Form evaluation request is not canonical: {error}"))
    })?;
    let expected_compiler_revision = semantic_core.compiler_revision();
    let response = tokio::task::spawn_blocking(move || semantic_core.evaluate(&request))
        .await
        .map_err(|error| {
            ApplicationError::Internal(format!("Form evaluator task failed: {error}"))
        })?
        .map_err(|error| ApplicationError::Unavailable(error.to_string()))?;
    let response: EvaluationResponseEnvelope =
        serde_json::from_slice(&response).map_err(|error| {
            ApplicationError::Internal(format!("Form evaluator response is invalid JSON: {error}"))
        })?;
    if response.api_version != EVALUATE_RESPONSE_API_VERSION
        || response.compiler_revision != expected_compiler_revision
    {
        return Err(ApplicationError::Internal(
            "Form evaluator returned an incompatible protocol identity".into(),
        ));
    }
    if !response.ok {
        return Err(ApplicationError::Invalid(evaluation_failure(&response)));
    }
    response.value.ok_or_else(|| {
        ApplicationError::Internal("successful Form evaluation omitted the accepted value".into())
    })
}

fn evaluation_failure(response: &EvaluationResponseEnvelope) -> String {
    let Some(error) = response.errors.first() else {
        return "Form submission evaluation failed without a diagnostic".into();
    };
    format!(
        "Form submission evaluation failed ({}) at {}: {}",
        error.code, error.path, error.message
    )
}
