use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::presentation::resource_access_evaluator;
use crate::modules::shared_kernel::domain::PrincipalId;
use crate::modules::workflow::domain::{ONTOLOGY_MAX_ACL_BYTES, WORKFLOW_GOAL_MAX_ACL_BYTES};
use crate::presentation::A3S_ACL_MEDIA_TYPE;
use a3s_boot::{BootError, BootRequest, Result};
use uuid::Uuid;

pub(super) fn resource_access(request: &BootRequest) -> Result<ResourceAccessEvaluator> {
    resource_access_evaluator(&request.require_auth_principal()?)
}

pub(super) fn actor_principal_id(request: &BootRequest) -> Result<PrincipalId> {
    let principal = request.require_auth_principal()?;
    Uuid::parse_str(principal.subject())
        .map(PrincipalId::from_uuid)
        .map_err(|error| {
            BootError::Internal(format!(
                "authenticated principal identity is invalid: {error}"
            ))
        })
}

pub(super) fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    Ok((idempotency_key, request_id(request)?))
}

pub(super) fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

pub(super) fn ontology_acl(request: &BootRequest) -> Result<String> {
    let media_type = request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !media_type.is_some_and(|value| value.eq_ignore_ascii_case(A3S_ACL_MEDIA_TYPE)) {
        return Err(BootError::UnsupportedMediaType(format!(
            "Ontology revisions require {A3S_ACL_MEDIA_TYPE}"
        )));
    }
    if request.body().is_empty() {
        return Err(BootError::BadRequest(
            "Ontology ACL body is required".into(),
        ));
    }
    if request.body().len() > ONTOLOGY_MAX_ACL_BYTES {
        return Err(BootError::PayloadTooLarge(format!(
            "Ontology ACL exceeds {ONTOLOGY_MAX_ACL_BYTES} bytes"
        )));
    }
    std::str::from_utf8(request.body())
        .map(str::to_owned)
        .map_err(|_| BootError::BadRequest("Ontology ACL must be valid UTF-8".into()))
}

pub(super) fn workflow_goal_acl(request: &BootRequest) -> Result<String> {
    acl_body(request, "Workflow goal", WORKFLOW_GOAL_MAX_ACL_BYTES)
}

fn acl_body(request: &BootRequest, label: &str, maximum_bytes: usize) -> Result<String> {
    let media_type = request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !media_type.is_some_and(|value| value.eq_ignore_ascii_case(A3S_ACL_MEDIA_TYPE)) {
        return Err(BootError::UnsupportedMediaType(format!(
            "{label} requires {A3S_ACL_MEDIA_TYPE}"
        )));
    }
    if request.body().is_empty() {
        return Err(BootError::BadRequest(format!(
            "{label} ACL body is required"
        )));
    }
    if request.body().len() > maximum_bytes {
        return Err(BootError::PayloadTooLarge(format!(
            "{label} ACL exceeds {maximum_bytes} bytes"
        )));
    }
    std::str::from_utf8(request.body())
        .map(str::to_owned)
        .map_err(|_| BootError::BadRequest(format!("{label} ACL must be valid UTF-8")))
}

pub(super) fn expected_version(request: &BootRequest) -> Result<u64> {
    let expected_version = request
        .header("x-a3s-expected-version")
        .ok_or_else(|| BootError::BadRequest("x-a3s-expected-version header is required".into()))?
        .parse::<u64>()
        .map_err(|_| {
            BootError::BadRequest("x-a3s-expected-version must be a positive integer".into())
        })?;
    if expected_version == 0 {
        return Err(BootError::BadRequest(
            "x-a3s-expected-version must be a positive integer".into(),
        ));
    }
    Ok(expected_version)
}

pub(super) fn revision_control(request: &BootRequest) -> Result<(u64, Option<String>)> {
    let expected_version = request
        .header("x-a3s-expected-version")
        .ok_or_else(|| BootError::BadRequest("x-a3s-expected-version header is required".into()))?
        .parse::<u64>()
        .map_err(|_| {
            BootError::BadRequest("x-a3s-expected-version must be a positive integer".into())
        })?;
    if expected_version == 0 {
        return Err(BootError::BadRequest(
            "x-a3s-expected-version must be a positive integer".into(),
        ));
    }
    let migration_rule_id = request.header("x-a3s-migration-rule").map(str::to_owned);
    if migration_rule_id.as_deref().is_some_and(|value| {
        value.is_empty()
            || value.len() > 96
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }) {
        return Err(BootError::BadRequest(
            "x-a3s-migration-rule must be a portable Ontology rule ID".into(),
        ));
    }
    Ok((expected_version, migration_rule_id))
}
