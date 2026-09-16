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
use a3s_boot::{
    controller, get, post, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus,
    Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn preview_management_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let controller = Arc::new(PreviewManagementCommandsController { bus }).controller()?;
    organization_tenant_build_write_controller(controller)
}

pub fn preview_management_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let controller = Arc::new(PreviewManagementQueriesController { bus }).controller()?;
    organization_tenant_cloud_read_controller(controller)
}

#[derive(Debug, Clone)]
struct PreviewManagementCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct PreviewManagementQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
impl PreviewManagementCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies",
        raw
    )]
    async fn accept_policy(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AcceptPullRequestPreviewPolicyRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
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
}

#[controller("/organizations")]
impl PreviewManagementQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies/{source_subscription_id}",
        raw
    )]
    async fn current_policy(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies/{source_subscription_id}/revisions",
        raw
    )]
    async fn list_revisions(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies/{source_subscription_id}/revisions/{preview_policy_revision_id}",
        raw
    )]
    async fn get_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetAcceptedPullRequestPreviewPolicyRevision {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                source_environment_id: environment_id(&request)?,
                source_subscription_id: source_subscription_id(&request)?,
                preview_policy_revision_id: PullRequestPreviewPolicyRevisionId::from_uuid(
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

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-previews/{source_subscription_id}/pull-requests/{pull_request_id}",
        raw
    )]
    async fn get_preview(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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
            Ok(preview) => BootResponse::json(&PullRequestPreviewResponse::from(preview)),
            Err(error) => application_error_response(error, request_id),
        }
    }
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

#[cfg(test)]
mod nest_macro_preview_management_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn preview_management_controllers_register_scoped_routes_via_nest_macros() {
        let commands = preview_management_commands_controller(Arc::new(CommandBus::new()))
            .expect("preview management nest command controller");
        assert_eq!(commands.prefix(), DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX);
        assert_eq!(commands.routes().len(), 1);
        assert_eq!(commands.routes()[0].method(), HttpMethod::Post);
        assert_eq!(
            commands.routes()[0].path(),
            format!(
                "{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE}"
            )
        );

        let queries = preview_management_queries_controller(Arc::new(QueryBus::new()))
            .expect("preview management nest query controller");
        assert_eq!(queries.prefix(), DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX);
        assert_eq!(queries.routes().len(), 4);
        assert_eq!(queries.routes()[0].method(), HttpMethod::Get);
        assert_eq!(
            queries.routes()[0].path(),
            format!(
                "{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE}"
            )
        );
        assert_eq!(
            queries.routes()[3].path(),
            format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{PULL_REQUEST_PREVIEW_ITEM_ROUTE}")
        );
        assert_eq!(
            queries.routes()[0]
                .metadata()
                .get("auth.scopes")
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!(["cloud:read"])
        );
    }
}
