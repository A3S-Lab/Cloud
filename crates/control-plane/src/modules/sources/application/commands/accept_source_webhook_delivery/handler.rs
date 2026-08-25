use super::{AcceptSourceWebhookDelivery, AcceptSourceWebhookDeliveryResult};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::domain::{
    AcceptSourceWebhook, IGithubConnectionRepository, ISourceWebhookRepository,
    NewSourceWebhookDelivery, SourcePullRequestWebhookDelivery, SourcePushWebhookDelivery,
    SourceWebhookDelivery, SourceWebhookPayload, VerifiedRepositoryWebhook,
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
                .find_authoritative_by_installation(command.webhook.installation_id())
                .await
            {
                Ok(connection) => connection.map(|connection| connection.id),
                Err(error) => return Ok(Err(error.into())),
            };
            let correlation_id = command.request_id;
            let delivery = match SourceWebhookDelivery::accept(new_delivery(command)) {
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
                    correlation_id,
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
                pull_request_changes: accepted.pull_request_changes,
            }))
        })
    }
}

fn new_delivery(command: AcceptSourceWebhookDelivery) -> NewSourceWebhookDelivery {
    let AcceptSourceWebhookDelivery {
        webhook,
        received_at,
        request_id: _,
    } = command;
    match webhook {
        VerifiedRepositoryWebhook::Push(push) => NewSourceWebhookDelivery {
            provider: push.provider,
            delivery_id: push.delivery_id,
            installation_id: push.installation_id,
            repository: push.repository,
            payload: SourceWebhookPayload::Push(SourcePushWebhookDelivery {
                reference: push.reference,
                commit_sha: push.commit_sha,
            }),
            payload_digest: push.payload_digest,
            received_at,
        },
        VerifiedRepositoryWebhook::PullRequest(change) => NewSourceWebhookDelivery {
            provider: change.provider,
            delivery_id: change.delivery_id,
            installation_id: change.installation_id,
            repository: change.base_repository,
            payload: SourceWebhookPayload::PullRequest(SourcePullRequestWebhookDelivery {
                base_reference: change.base_reference,
                head_repository: change.head_repository,
                head_reference: change.head_reference,
                head_commit_sha: change.head_commit_sha,
                pull_request_id: change.pull_request_id,
                pull_request_number: change.pull_request_number,
                kind: change.kind,
                merged: change.merged,
                provider_created_at: change.provider_created_at,
                provider_updated_at: change.provider_updated_at,
            }),
            payload_digest: change.payload_digest,
            received_at,
        },
    }
}
