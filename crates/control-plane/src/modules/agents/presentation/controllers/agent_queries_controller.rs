use super::request::request_id;
use crate::modules::agents::application::{
    GetAgentConversation, GetAgentExecution, GetAgentExecutionChangeSet, GetAgentExecutionEvents,
    ListAgentConversations, ListAgentExecutions,
};
use crate::modules::agents::presentation::dto::{
    AgentConversationResponse, AgentExecutionChangeSetResponse, AgentExecutionEventPageResponse,
    AgentExecutionResponse,
};
use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::{
    application_error_response, decode_sequence_cursor, default_live_sequence_limit,
    resolve_sequence_cursor, sequence_stream_error, stream_sequence_pages,
    MAX_LIVE_SEQUENCE_RECORDS,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
    SseStream,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn agent_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_conversation_bus = Arc::clone(&bus);
    let list_executions_bus = Arc::clone(&bus);
    let get_execution_bus = Arc::clone(&bus);
    let get_change_set_bus = Arc::clone(&bus);
    let get_events_bus = Arc::clone(&bus);
    let stream_events_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/agent-conversations",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = request
                        .optional_query_value_as::<usize>("limit")?
                        .unwrap_or(50);
                    match bus
                        .execute(ListAgentConversations {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            limit,
                        })
                        .await?
                    {
                        Ok(conversations) => BootResponse::json(
                            &conversations
                                .into_iter()
                                .map(AgentConversationResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/agent-conversations/{conversation_id}",
                move |request: BootRequest| {
                let bus = Arc::clone(&get_conversation_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAgentConversation {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            conversation_id: AgentConversationId::from_uuid(
                                request.param_as::<Uuid>("conversation_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(conversation) => {
                            BootResponse::json(&AgentConversationResponse::from(conversation))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/agent-conversations/{conversation_id}/executions",
                move |request: BootRequest| {
                let bus = Arc::clone(&list_executions_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = request
                        .optional_query_value_as::<usize>("limit")?
                        .unwrap_or(50);
                    match bus
                        .execute(ListAgentExecutions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            conversation_id: AgentConversationId::from_uuid(
                                request.param_as::<Uuid>("conversation_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                            limit,
                        })
                        .await?
                    {
                        Ok(executions) => BootResponse::json(
                            &executions
                                .into_iter()
                                .map(AgentExecutionResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/agent-executions/{execution_id}",
                move |request: BootRequest| {
                let bus = Arc::clone(&get_execution_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAgentExecution {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            execution_id: AgentExecutionId::from_uuid(
                                request.param_as::<Uuid>("execution_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(execution) => {
                            BootResponse::json(&AgentExecutionResponse::from(execution))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/agent-executions/{execution_id}/changes",
                move |request: BootRequest| {
                let bus = Arc::clone(&get_change_set_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAgentExecutionChangeSet {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            execution_id: AgentExecutionId::from_uuid(
                                request.param_as::<Uuid>("execution_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(change_set) => BootResponse::json(
                            &AgentExecutionChangeSetResponse::from(change_set),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/agent-conversations/{conversation_id}/events",
                move |request: BootRequest| {
                let bus = Arc::clone(&get_events_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let parameters: AgentEventsQuery = request.query()?;
                    match bus
                        .execute(GetAgentExecutionEvents {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            conversation_id: AgentConversationId::from_uuid(
                                request.param_as::<Uuid>("conversation_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                            after_sequence: decode_sequence_cursor(
                                parameters.cursor.as_deref(),
                                "Agent event",
                            )?,
                            limit: parameters.limit,
                        })
                        .await?
                    {
                        Ok(page) => {
                            BootResponse::json(&AgentExecutionEventPageResponse::from(page))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::sse(
                "/{organization_id}/agent-conversations/{conversation_id}/events/stream",
                move |request: BootRequest| {
                let bus = Arc::clone(&stream_events_bus);
                async move {
                    let parameters: AgentLiveEventsQuery = request.query()?;
                    if parameters.limit == 0 || parameters.limit > MAX_LIVE_SEQUENCE_RECORDS {
                        return Err(BootError::BadRequest(format!(
                            "live Agent event limit must be between 1 and {MAX_LIVE_SEQUENCE_RECORDS}"
                        )));
                    }
                    let after_sequence = resolve_sequence_cursor(
                        &request,
                        parameters.cursor.as_deref(),
                        "Agent event",
                    )?;
                    agent_event_stream(
                        bus,
                        GetAgentExecutionEvents {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            conversation_id: AgentConversationId::from_uuid(
                                request.param_as::<Uuid>("conversation_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                            after_sequence,
                            limit: usize::from(parameters.limit),
                        },
                    )
                    .await
                }
                },
            )?,
            DeferredResourceScope::Project,
        )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentEventsQuery {
    cursor: Option<String>,
    #[serde(default = "default_event_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentLiveEventsQuery {
    cursor: Option<String>,
    #[serde(default = "default_live_sequence_limit")]
    limit: u16,
}

const fn default_event_limit() -> usize {
    100
}

async fn agent_event_stream(
    bus: Arc<QueryBus>,
    query: GetAgentExecutionEvents,
) -> Result<SseStream> {
    stream_sequence_pages(
        query,
        move |query| {
            let bus = Arc::clone(&bus);
            async move {
                bus.execute(query)
                    .await?
                    .map(AgentExecutionEventPageResponse::from)
                    .map_err(|error| sequence_stream_error(error, "live Agent event query failed"))
            }
        },
        |query, sequence| query.after_sequence = Some(sequence),
        "Agent event",
    )
    .await
}
