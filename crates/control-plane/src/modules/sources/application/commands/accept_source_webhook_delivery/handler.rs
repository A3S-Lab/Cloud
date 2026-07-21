use super::{AcceptSourceWebhookDelivery, AcceptSourceWebhookDeliveryResult};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::domain::{
    AcceptSourceWebhook, IGithubConnectionRepository, ISourceWebhookRepository,
    NewSourceWebhookDelivery, SourceWebhookDelivery,
};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct AcceptSourceWebhookDeliveryHandler {
    webhooks: Arc<dyn ISourceWebhookRepository>,
    connections: Arc<dyn IGithubConnectionRepository>,
}

impl AcceptSourceWebhookDeliveryHandler {
    pub fn new(
        webhooks: Arc<dyn ISourceWebhookRepository>,
        connections: Arc<dyn IGithubConnectionRepository>,
    ) -> Self {
        Self {
            webhooks,
            connections,
        }
    }
}

impl CommandHandler<AcceptSourceWebhookDelivery> for AcceptSourceWebhookDeliveryHandler {
    fn execute(
        &self,
        command: AcceptSourceWebhookDelivery,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptSourceWebhookDeliveryResult>>,
    > {
        let webhooks = Arc::clone(&self.webhooks);
        let connections = Arc::clone(&self.connections);
        Box::pin(async move {
            let authoritative_connection_id = match connections
                .find_authoritative_by_installation(command.push.installation_id)
                .await
            {
                Ok(connection) => connection.map(|connection| connection.id),
                Err(error) => return Ok(Err(error.into())),
            };
            let delivery = match SourceWebhookDelivery::accept(NewSourceWebhookDelivery {
                provider: command.push.provider,
                delivery_id: command.push.delivery_id,
                repository: command.push.repository,
                installation_id: command.push.installation_id,
                reference: command.push.reference,
                commit_sha: command.push.commit_sha,
                payload_digest: command.push.payload_digest,
                received_at: command.received_at,
            }) {
                Ok(value) => value,
                Err(error) => {
                    return Ok(Err(
                        crate::modules::shared_kernel::application::ApplicationError::Invalid(
                            error,
                        ),
                    ))
                }
            };
            let accepted = match webhooks
                .accept_delivery(AcceptSourceWebhook {
                    delivery,
                    authoritative_connection_id,
                    correlation_id: command.request_id,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(AcceptSourceWebhookDeliveryResult {
                delivery: accepted.delivery,
                replayed: accepted.replayed,
                revisions: accepted.revisions,
            }))
        })
    }
}
