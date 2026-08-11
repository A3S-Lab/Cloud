use super::{arguments, tool_result};
use crate::modules::forms::presentation::{
    FormDraftMutationResponse, FormDraftResponse, FormPublicationMutationResponse,
    FormReleaseResponse,
};
use crate::modules::forms::{
    CreateFormDraft, GetFormDraft, GetFormRelease, ListFormDrafts, ListFormReleases,
    PublishFormRelease, ReviseFormDraft, CLOUD_FORM_DOCUMENT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{BootError, CommandBus, QueryBus, Result};
use a3s_form_core::{canonicalize_value, CanonicalValue};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormDraftArguments {
    form_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormReleaseArguments {
    form_id: Uuid,
    release_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListFormDraftsArguments {
    project_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateFormDraftArguments {
    project_id: Uuid,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(deserialize_with = "deserialize_form_document")]
    document: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseFormDraftArguments {
    form_id: Uuid,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(deserialize_with = "deserialize_form_document")]
    document: String,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishFormReleaseArguments {
    form_id: Uuid,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

pub async fn create_draft(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateFormDraftArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateFormDraft {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            name: arguments.name,
            description: arguments.description,
            document_json: arguments.document,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            let response =
                FormDraftMutationResponse::try_from(result).map_err(BootError::Internal)?;
            tool_result::success(status, response, request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revise_draft(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ReviseFormDraftArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ReviseFormDraft {
            organization_id,
            form_id: FormId::from_uuid(arguments.form_id),
            expected_version: arguments.expected_version,
            name: arguments.name,
            description: arguments.description,
            document_json: arguments.document,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            let response =
                FormDraftMutationResponse::try_from(result).map_err(BootError::Internal)?;
            tool_result::success(status, response, request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn publish_release(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: PublishFormReleaseArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(PublishFormRelease {
            organization_id,
            form_id: FormId::from_uuid(arguments.form_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            let response =
                FormPublicationMutationResponse::try_from(result).map_err(BootError::Internal)?;
            tool_result::success(status, response, request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_drafts(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListFormDraftsArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListFormDrafts {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
        })
        .await?
    {
        Ok(values) => {
            let values = values
                .into_iter()
                .map(FormDraftResponse::try_from)
                .collect::<std::result::Result<Vec<_>, String>>()
                .map_err(BootError::Internal)?;
            tool_result::success(200, values, request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_draft(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: FormDraftArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetFormDraft {
            organization_id,
            form_id: FormId::from_uuid(arguments.form_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(
            200,
            FormDraftResponse::try_from(value).map_err(BootError::Internal)?,
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_releases(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: FormDraftArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListFormReleases {
            organization_id,
            form_id: FormId::from_uuid(arguments.form_id),
        })
        .await?
    {
        Ok(values) => {
            let values = values
                .into_iter()
                .map(FormReleaseResponse::try_from)
                .collect::<std::result::Result<Vec<_>, String>>()
                .map_err(BootError::Internal)?;
            tool_result::success(200, values, request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_release(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: FormReleaseArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetFormRelease {
            organization_id,
            form_id: FormId::from_uuid(arguments.form_id),
            release_id: FormReleaseId::from_uuid(arguments.release_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(
            200,
            FormReleaseResponse::try_from(value).map_err(BootError::Internal)?,
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

fn deserialize_form_document<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let document = CanonicalValue::deserialize(deserializer)?;
    if document.as_object().is_none() {
        return Err(D::Error::custom("Form document must be a JSON object"));
    }
    let canonical = canonicalize_value(&document)
        .map_err(|error| D::Error::custom(format!("Form document is invalid: {error}")))?;
    if canonical.len() > CLOUD_FORM_DOCUMENT_MAX_BYTES {
        return Err(D::Error::custom(format!(
            "Form document exceeds its {CLOUD_FORM_DOCUMENT_MAX_BYTES}-byte canonical bound"
        )));
    }
    String::from_utf8(canonical)
        .map_err(|_| D::Error::custom("Form document canonical JSON is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn form_arguments_are_closed_and_transport_canonical_documents() {
        let form_id = Uuid::new_v4();
        let arguments = arguments::parse::<ReviseFormDraftArguments>(json!({
            "formId": form_id,
            "name": "Approval",
            "document": {"z": 1, "a": true},
            "expectedVersion": 1,
            "idempotencyKey": "revise-approval"
        }))
        .expect("valid Form revision arguments");
        assert_eq!(arguments.document, r#"{"a":true,"z":1}"#);
        assert_eq!(arguments.description, "");

        for value in [
            json!({
                "formId": form_id,
                "name": "Approval",
                "document": [],
                "expectedVersion": 1,
                "idempotencyKey": "revise-approval"
            }),
            json!({
                "formId": form_id,
                "name": "Approval",
                "document": {},
                "expectedVersion": 0,
                "idempotencyKey": "revise-approval"
            }),
            json!({
                "formId": form_id,
                "name": "Approval",
                "document": {},
                "expectedVersion": 1,
                "idempotencyKey": "revise-approval",
                "organizationId": Uuid::new_v4()
            }),
        ] {
            assert!(arguments::parse::<ReviseFormDraftArguments>(value).is_err());
        }
    }
}
