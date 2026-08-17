use super::tool_result;
use crate::modules::durable_cells::presentation::{
    DurableCellApplicationMutationResponse, DurableCellApplicationRecordResponse,
    DurableCellApplicationResponse, DurableCellApplicationRevisionResponse,
    DurableCellDeploymentResponse, DurableCellRoutePublicationResponse,
};
use crate::modules::durable_cells::{
    CreateDurableCellApplication, DeployDurableCellApplicationFromAcl, GetDurableCellApplication,
    GetDurableCellApplicationRevision, ListDurableCellApplicationRevisions,
    ListDurableCellApplications, PublishDurableCellApplicationRoute, ReviseDurableCellApplication,
    StartDurableCellApplication, StopDurableCellApplication,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    GatewayScopeId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDurableCellApplicationArguments {
    project_id: Uuid,
    environment_id: Uuid,
    name: String,
    definition_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseDurableCellApplicationArguments {
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    definition_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDurableCellApplicationStateArguments {
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListDurableCellApplicationsArguments {
    project_id: Uuid,
    environment_id: Uuid,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DurableCellApplicationArguments {
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListDurableCellApplicationRevisionsArguments {
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DurableCellApplicationRevisionArguments {
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeployDurableCellApplicationArguments {
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    revision_id: Uuid,
    service_profile_acl: String,
    storage_provider_profile_acl: Option<String>,
    provider_workload_acl: String,
    storage_binding_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishDurableCellApplicationRouteArguments {
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    revision_id: Uuid,
    service_profile_acl: String,
    gateway_scope_id: Uuid,
    domain_claim_id: Uuid,
    hostname: String,
    path_prefix: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

pub async fn create_application(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateDurableCellApplicationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateDurableCellApplication {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            name: arguments.name,
            definition_acl: arguments.definition_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            DurableCellApplicationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revise_application(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ReviseDurableCellApplicationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ReviseDurableCellApplication {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            application_id: DurableCellApplicationId::from_uuid(arguments.application_id),
            expected_version: arguments.expected_version,
            definition_acl: arguments.definition_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            DurableCellApplicationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn set_application_state(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: SetDurableCellApplicationStateArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
    start: bool,
) -> Result<Value> {
    let project_id = ProjectId::from_uuid(arguments.project_id);
    let environment_id = EnvironmentId::from_uuid(arguments.environment_id);
    let application_id = DurableCellApplicationId::from_uuid(arguments.application_id);
    let result = if start {
        bus.execute(StartDurableCellApplication {
            organization_id,
            project_id,
            environment_id,
            application_id,
            expected_version: arguments.expected_version,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    } else {
        bus.execute(StopDurableCellApplication {
            organization_id,
            project_id,
            environment_id,
            application_id,
            expected_version: arguments.expected_version,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    };
    match result {
        Ok(result) => tool_result::success(
            200,
            DurableCellApplicationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_applications(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListDurableCellApplicationsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListDurableCellApplications {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            limit: arguments.limit,
            resource_access,
        })
        .await?
    {
        Ok(applications) => tool_result::success(
            200,
            applications
                .into_iter()
                .map(DurableCellApplicationResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_application(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: DurableCellApplicationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetDurableCellApplication {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            application_id: DurableCellApplicationId::from_uuid(arguments.application_id),
            resource_access,
        })
        .await?
    {
        Ok(record) => tool_result::success(
            200,
            DurableCellApplicationRecordResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_revisions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListDurableCellApplicationRevisionsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListDurableCellApplicationRevisions {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            application_id: DurableCellApplicationId::from_uuid(arguments.application_id),
            limit: arguments.limit,
            resource_access,
        })
        .await?
    {
        Ok(revisions) => tool_result::success(
            200,
            revisions
                .into_iter()
                .map(DurableCellApplicationRevisionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: DurableCellApplicationRevisionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetDurableCellApplicationRevision {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            application_id: DurableCellApplicationId::from_uuid(arguments.application_id),
            revision_id: DurableCellApplicationRevisionId::from_uuid(arguments.revision_id),
            resource_access,
        })
        .await?
    {
        Ok(revision) => tool_result::success(
            200,
            DurableCellApplicationRevisionResponse::from(revision),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn deploy_application(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: DeployDurableCellApplicationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(DeployDurableCellApplicationFromAcl {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            application_id: DurableCellApplicationId::from_uuid(arguments.application_id),
            application_revision_id: DurableCellApplicationRevisionId::from_uuid(
                arguments.revision_id,
            ),
            service_profile_acl: arguments.service_profile_acl,
            storage_provider_profile_acl: arguments.storage_provider_profile_acl,
            provider_workload_acl: arguments.provider_workload_acl,
            storage_binding_acl: arguments.storage_binding_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            DurableCellDeploymentResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn publish_route(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: PublishDurableCellApplicationRouteArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(PublishDurableCellApplicationRoute {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            application_id: DurableCellApplicationId::from_uuid(arguments.application_id),
            application_revision_id: DurableCellApplicationRevisionId::from_uuid(
                arguments.revision_id,
            ),
            service_profile_acl: arguments.service_profile_acl,
            gateway_scope_id: GatewayScopeId::from_uuid(arguments.gateway_scope_id),
            domain_claim_id: DomainClaimId::from_uuid(arguments.domain_claim_id),
            hostname: arguments.hostname,
            path_prefix: arguments.path_prefix,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => {
            let status = if result.route.publication.replayed {
                200
            } else {
                201
            };
            tool_result::success(
                status,
                DurableCellRoutePublicationResponse::from(result),
                request_id,
            )
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}
