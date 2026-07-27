use super::arguments::OperationListArguments;
use super::tool_result;
use crate::modules::operations::presentation::OperationListItemResponse;
use crate::modules::operations::ListOperations;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{QueryBus, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn list_operations(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: OperationListArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListOperations {
            organization_id,
            limit: arguments.limit,
        })
        .await?
    {
        Ok(operations) => tool_result::success(
            200,
            operations
                .into_iter()
                .map(OperationListItemResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
