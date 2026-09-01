use super::preview_management_dto::{
    AcceptPullRequestPreviewPolicyRequest, AcceptedPullRequestPreviewPolicyRevisionResponse,
    PullRequestPreviewPolicyMutationResponse, PullRequestPreviewResponse,
};
use super::request::{environment_id, organization_id, project_id, workflow_access};
use super::routes::{
    DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX, PULL_REQUEST_PREVIEW_ITEM_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE, PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE,
};
use crate::modules::developer_workflows::{
    AcceptPullRequestPreviewPolicy, GetAcceptedPullRequestPreviewPolicyRevision,
    GetCurrentAcceptedPullRequestPreviewPolicyRevision, GetPullRequestPreview,
    ListAcceptedPullRequestPreviewPolicyRevisions, DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT,
};
use crate::modules::shared_kernel::domain::{
    PullRequestPreviewPolicyRevisionId, SourceSubscriptionId,
};
use crate::presentation::{
    actor_principal_id, application_error_response, organization_tenant_build_write_controller,
    organization_tenant_cloud_read_controller, request_id, request_identity,
};
use a3s_boot::{BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn preview_management_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX)?.post(
        PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE,
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: AcceptPullRequestPreviewPolicyRequest =
                    request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(AcceptPullRequestPreviewPolicy {
                        organization_id: organization_id(&request)?,
                        project_id: project_id(&request)?,
                        source_environment_id: environment_id(&request)?,
                        source_subscription_id: SourceSubscriptionId::from_uuid(
                            body.source_subscription_id,
                        ),
                        policy_acl: body.policy_acl,
                        access: workflow_access(&request)?,
                        actor_principal_id: actor_principal_id(&request)?,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        BootResponse::json_with_status(
                            status,
                            &PullRequestPreviewPolicyMutationResponse::from(result),
                        )
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    organization_tenant_build_write_controller(controller)
}

pub fn preview_management_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let current_bus = Arc::clone(&bus);
    let list_bus = Arc::clone(&bus);
    let revision_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX)?
        .get(
            PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&current_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetCurrentAcceptedPullRequestPreviewPolicyRevision {
                            organization_id: organization_id(&request)?,
                            project_id: project_id(&request)?,
                            source_environment_id: environment_id(&request)?,
                            source_subscription_id: source_subscription_id(&request)?,
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(revision) => BootResponse::json(
                            &AcceptedPullRequestPreviewPolicyRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListAcceptedPullRequestPreviewPolicyRevisions {
                            organization_id: organization_id(&request)?,
                            project_id: project_id(&request)?,
                            source_environment_id: environment_id(&request)?,
                            source_subscription_id: source_subscription_id(&request)?,
                            limit: revision_list_limit(&request)?,
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(revisions) => BootResponse::json(
                            &revisions
                                .into_iter()
                                .map(AcceptedPullRequestPreviewPolicyRevisionResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&revision_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAcceptedPullRequestPreviewPolicyRevision {
                            organization_id: organization_id(&request)?,
                            project_id: project_id(&request)?,
                            source_environment_id: environment_id(&request)?,
                            source_subscription_id: source_subscription_id(&request)?,
                            preview_policy_revision_id:
                                PullRequestPreviewPolicyRevisionId::from_uuid(
                                    request.param_as::<Uuid>("preview_policy_revision_id")?,
                                ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(revision) => BootResponse::json(
                            &AcceptedPullRequestPreviewPolicyRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            PULL_REQUEST_PREVIEW_ITEM_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetPullRequestPreview {
                            organization_id: organization_id(&request)?,
                            project_id: project_id(&request)?,
                            source_environment_id: environment_id(&request)?,
                            source_subscription_id: source_subscription_id(&request)?,
                            pull_request_id: request.param_as::<u64>("pull_request_id")?,
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(preview) => {
                            BootResponse::json(&PullRequestPreviewResponse::from(preview))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?;
    organization_tenant_cloud_read_controller(controller)
}

fn source_subscription_id(request: &BootRequest) -> Result<SourceSubscriptionId> {
    Ok(SourceSubscriptionId::from_uuid(
        request.param_as::<Uuid>("source_subscription_id")?,
    ))
}

fn revision_list_limit(request: &BootRequest) -> Result<usize> {
    Ok(request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT))
}
