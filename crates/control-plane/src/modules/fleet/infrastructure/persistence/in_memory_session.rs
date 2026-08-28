use super::InMemoryNodeRepository;
use crate::modules::fleet::domain::repositories::INodeProtocolSessionRepository;
use crate::modules::fleet::domain::value_objects::{
    NodeProtocolNegotiation, NodeProtocolNegotiationOutcome, NodeState,
};
use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
use async_trait::async_trait;

#[async_trait]
impl INodeProtocolSessionRepository for InMemoryNodeRepository {
    async fn negotiate(
        &self,
        negotiation: NodeProtocolNegotiation,
    ) -> Result<NodeProtocolNegotiationOutcome, RepositoryError> {
        let node_id = NodeId::from_uuid(negotiation.hello().node_id);
        let mut state = self.state.write().await;
        let node = state
            .nodes
            .values()
            .find(|node| node.id == node_id)
            .ok_or(RepositoryError::NotFound)?;
        if node.state == NodeState::Revoked {
            return Err(RepositoryError::NotFound);
        }
        if node.agent_instance_id != negotiation.hello().agent_instance_id {
            return Err(RepositoryError::Forbidden(
                "node session Agent instance does not match the enrolled node".into(),
            ));
        }

        let outcome = negotiation
            .apply(state.protocol_sessions.get(&node_id))
            .map_err(|error| RepositoryError::Conflict(error.to_string()))?;
        if !outcome.replayed() {
            state
                .protocol_sessions
                .insert(node_id, outcome.record().clone());
        }
        Ok(outcome)
    }
}

#[cfg(test)]
#[path = "in_memory_session_tests.rs"]
mod tests;
