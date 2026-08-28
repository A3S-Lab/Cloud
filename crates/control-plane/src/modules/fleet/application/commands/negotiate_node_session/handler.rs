use super::{NegotiateNodeSession, NegotiateNodeSessionResult};
use crate::modules::fleet::domain::repositories::INodeProtocolSessionRepository;
use crate::modules::fleet::domain::value_objects::{NodeProtocolNegotiation, NodeProtocolPolicy};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use a3s_cloud_contracts::NodeSessionSelection;
use chrono::Duration;
use std::sync::Arc;
use uuid::Uuid;

pub struct NegotiateNodeSessionHandler {
    sessions: Arc<dyn INodeProtocolSessionRepository>,
    policy: NodeProtocolPolicy,
    selection_lifetime: Duration,
}

impl NegotiateNodeSessionHandler {
    pub fn new(
        sessions: Arc<dyn INodeProtocolSessionRepository>,
        policy: NodeProtocolPolicy,
        selection_lifetime: Duration,
    ) -> Result<Self, String> {
        if selection_lifetime <= Duration::zero()
            || selection_lifetime > Duration::hours(NodeSessionSelection::MAX_LIFETIME_HOURS)
        {
            return Err("node protocol selection lifetime is invalid".into());
        }
        Ok(Self {
            sessions,
            policy,
            selection_lifetime,
        })
    }
}

impl CommandHandler<NegotiateNodeSession> for NegotiateNodeSessionHandler {
    fn execute(
        &self,
        command: NegotiateNodeSession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<NegotiateNodeSessionResult>>>
    {
        let sessions = Arc::clone(&self.sessions);
        let policy = self.policy.clone();
        let selection_lifetime = self.selection_lifetime;
        Box::pin(async move {
            if command.hello.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the node session hello".into(),
                )));
            }
            let negotiation = match NodeProtocolNegotiation::new(
                command.hello,
                policy,
                command.received_at,
                selection_lifetime,
                Uuid::now_v7(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error.to_string()))),
            };
            Ok(match sessions.negotiate(negotiation).await {
                Ok(outcome) => Ok(NegotiateNodeSessionResult {
                    selection: outcome.selection().clone(),
                    replayed: outcome.replayed(),
                }),
                Err(error) => Err(error.into()),
            })
        })
    }
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
