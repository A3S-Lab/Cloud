use super::tool_result;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
};
use crate::modules::workflow::presentation::{
    OntologyDiffResponse, OntologyMutationResponse, OntologyResponse, OntologyRevisionResponse,
    OntologyRevisionSummaryResponse,
};
use crate::modules::workflow::{
    CreateOntology, DiffOntologyRevisions, GetOntology, GetOntologyRevision, ListOntologies,
    ListOntologyRevisions, ReviseOntology,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OntologyArguments {
    ontology_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OntologyRevisionArguments {
    ontology_id: Uuid,
    revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OntologyDiffArguments {
    ontology_id: Uuid,
    from_revision_id: Uuid,
    to_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListOntologiesArguments {
    project_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateOntologyArguments {
    project_id: Uuid,
    acl: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseOntologyArguments {
    ontology_id: Uuid,
    acl: String,
    expected_version: u64,
    #[serde(default)]
    migration_rule_id: Option<String>,
    idempotency_key: String,
}

pub async fn create_ontology(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateOntologyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateOntology {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            acl: arguments.acl,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            OntologyMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revise_ontology(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ReviseOntologyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ReviseOntology {
            organization_id,
            ontology_id: OntologyId::from_uuid(arguments.ontology_id),
            acl: arguments.acl,
            expected_version: arguments.expected_version,
            migration_rule_id: arguments.migration_rule_id,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            OntologyMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_ontologies(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListOntologiesArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListOntologies {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
        })
        .await?
    {
        Ok(values) => tool_result::success(
            200,
            values
                .into_iter()
                .map(OntologyResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_ontology(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: OntologyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetOntology {
            organization_id,
            ontology_id: OntologyId::from_uuid(arguments.ontology_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, OntologyResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_revisions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: OntologyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListOntologyRevisions {
            organization_id,
            ontology_id: OntologyId::from_uuid(arguments.ontology_id),
        })
        .await?
    {
        Ok(values) => tool_result::success(
            200,
            values
                .into_iter()
                .map(OntologyRevisionSummaryResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: OntologyRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetOntologyRevision {
            organization_id,
            ontology_id: OntologyId::from_uuid(arguments.ontology_id),
            revision_id: OntologyRevisionId::from_uuid(arguments.revision_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, OntologyRevisionResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn diff_revisions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: OntologyDiffArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(DiffOntologyRevisions {
            organization_id,
            ontology_id: OntologyId::from_uuid(arguments.ontology_id),
            from_revision_id: OntologyRevisionId::from_uuid(arguments.from_revision_id),
            to_revision_id: OntologyRevisionId::from_uuid(arguments.to_revision_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, OntologyDiffResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
