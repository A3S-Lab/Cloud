use super::delivery_dto::{
    ApplicationMessageCitationMutationResponse, ApplicationMessageCitationResponse,
    CreateApplicationMessageCitationRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    CreateApplicationMessageCitation, GetApplicationMessageCitation,
    ListApplicationMessageCitationsBySession,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageCitationId, ApplicationMessageId, ApplicationSessionId,
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeChunkId, KnowledgeDocumentId,
    OrganizationId, ProjectId,
};
use crate::presentation::{actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_message_citation_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationMessageCitationCommandsController { bus }).controller()
}

pub fn application_message_citation_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationMessageCitationQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationMessageCitationCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationMessageCitationQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationMessageCitationCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-citations",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateApplicationMessageCitationRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(CreateApplicationMessageCitation {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                message_id: ApplicationMessageId::from_uuid(body.message_id),
                knowledge_base_id: KnowledgeBaseId::from_uuid(body.knowledge_base_id),
                knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(
                    body.knowledge_base_revision_id,
                ),
                knowledge_document_id: KnowledgeDocumentId::from_uuid(body.knowledge_document_id),
                knowledge_chunk_id: KnowledgeChunkId::from_uuid(body.knowledge_chunk_id),
                excerpt: body.excerpt,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                created_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ApplicationMessageCitationMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationMessageCitationQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-citations",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListApplicationMessageCitationsBySession {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(items) => BootResponse::json(
                &items
                    .into_iter()
                    .map(ApplicationMessageCitationResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-citations/{citation_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationMessageCitation {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                citation_id: ApplicationMessageCitationId::from_uuid(
                    request.param_as::<Uuid>("citation_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(citation) => {
                BootResponse::json(&ApplicationMessageCitationResponse::from(citation))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_message_citation_delivery_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn message_citation_controllers_register_scoped_routes_via_nest_macros() {
        let commands =
            application_message_citation_commands_controller(Arc::new(CommandBus::new()))
                .expect("message citation nest command controller");
        assert_eq!(commands.prefix(), "/organizations");
        assert_eq!(commands.routes().len(), 1);
        assert_eq!(commands.routes()[0].method(), HttpMethod::Post);

        let queries = application_message_citation_queries_controller(Arc::new(QueryBus::new()))
            .expect("message citation nest query controller");
        assert_eq!(queries.routes().len(), 2);
        assert_eq!(
            queries.routes()[1].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-citations/{citation_id}"
        );
        assert_eq!(
            queries.routes()[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::APPLICATION_WRITE])
        );
    }
}
