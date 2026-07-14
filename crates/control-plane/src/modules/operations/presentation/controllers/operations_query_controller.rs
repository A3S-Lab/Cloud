use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::operations::application::queries::list_operations::ListOperations;
use crate::modules::operations::presentation::dto::OperationListItemResponse;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, SseEvent,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

pub fn operations_query_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let snapshot_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/operations",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let limit = request
                        .optional_query_value_as::<usize>("limit")?
                        .unwrap_or(50);
                    if limit == 0 || limit > 200 {
                        return Err(BootError::BadRequest(
                            "limit must be between 1 and 200".into(),
                        ));
                    }
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListOperations {
                            organization_id,
                            limit,
                        })
                        .await?
                    {
                        Ok(operations) => BootResponse::json(
                            &operations
                                .into_iter()
                                .map(OperationListItemResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .sse(
            "/{organization_id}/operations/stream",
            move |request: BootRequest| {
                let bus = Arc::clone(&snapshot_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let last_event_id = request
                        .header("last-event-id")
                        .unwrap_or_default()
                        .to_owned();
                    Ok(async_stream::try_stream! {
                        let mut last_event_id = last_event_id;
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            interval.tick().await;
                            let operations = bus
                                .execute(ListOperations {
                                    organization_id,
                                    limit: 100,
                                })
                                .await?
                                .map_err(|_| BootError::Internal(
                                    "operation snapshot query failed".into(),
                                ))?
                                .into_iter()
                                .map(OperationListItemResponse::from)
                                .collect::<Vec<_>>();
                            let encoded = serde_json::to_vec(&operations)
                                .map_err(|error| BootError::Internal(error.to_string()))?;
                            let event_id = format!("sha256:{:x}", Sha256::digest(&encoded));
                            if event_id != last_event_id {
                                last_event_id.clone_from(&event_id);
                                yield SseEvent::new(String::from_utf8(encoded).map_err(|error| {
                                    BootError::Internal(error.to_string())
                                })?)
                                .with_event("snapshot")
                                .with_id(event_id)
                                .with_retry(2_000);
                            } else {
                                yield SseEvent::comment("keepalive");
                            }
                        }
                    })
                }
            },
        )
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
