use crate::modules::edge::application::{EdgeNodeScope, IEdgeNodeAccess};
use crate::modules::fleet::domain::repositories::INodeRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Fleet node authority.
#[derive(Clone)]
pub struct FleetEdgeNodeAccessAdapter {
    nodes: Arc<dyn INodeRepository>,
}

impl FleetEdgeNodeAccessAdapter {
    pub fn new(nodes: Arc<dyn INodeRepository>) -> Self {
        Self { nodes }
    }
}

#[async_trait]
impl IEdgeNodeAccess for FleetEdgeNodeAccessAdapter {
    async fn node_exists(&self, scope: EdgeNodeScope) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        match self
            .nodes
            .find(scope.organization_id(), scope.node_id())
            .await
        {
            Ok(node)
                if node.organization_id == scope.organization_id()
                    && node.id == scope.node_id()
                    && node.aggregate_version > 0 =>
            {
                Ok(true)
            }
            Ok(_) => Err(RepositoryError::Storage(
                "Fleet returned inconsistent Edge node evidence".into(),
            )),
            Err(RepositoryError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::fleet::domain::entities::{EnrollmentToken, Node, NodeCertificate};
    use crate::modules::fleet::domain::repositories::{
        NodeCertificateRotationCompletion, NodeCertificateRotationDraft,
        NodeCertificateRotationReservation, NodeEnrollmentDraft, NodeEnrollmentReservation,
        NodeHeartbeatUpdate, NodeStateChange,
    };
    use crate::modules::fleet::domain::value_objects::{
        EnrollmentTokenCredential, NodeCapabilities, NodeName,
    };
    use crate::modules::shared_kernel::domain::{
        EnrollmentTokenId, IdempotencyRequest, IdempotentWrite, NodeCertificateId, NodeId,
        OrganizationId,
    };
    use a3s_cloud_contracts::DomainEventEnvelope;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    struct StubNodeRepository {
        node: Option<Node>,
        find_calls: AtomicUsize,
    }

    #[async_trait]
    impl INodeRepository for StubNodeRepository {
        async fn issue_enrollment_token(
            &self,
            _token: EnrollmentToken,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<EnrollmentToken>, RepositoryError> {
            unreachable!("node access adapter never issues enrollment tokens")
        }

        async fn reserve_enrollment(
            &self,
            _credential: &EnrollmentTokenCredential,
            _draft: NodeEnrollmentDraft,
        ) -> Result<NodeEnrollmentReservation, RepositoryError> {
            unreachable!("node access adapter never reserves enrollment")
        }

        async fn complete_enrollment(
            &self,
            _token_id: EnrollmentTokenId,
            _node_id: NodeId,
            _request_digest: &str,
            _certificate: NodeCertificate,
            _event: DomainEventEnvelope,
        ) -> Result<NodeEnrollmentReservation, RepositoryError> {
            unreachable!("node access adapter never completes enrollment")
        }

        async fn reserve_certificate_rotation(
            &self,
            _organization_id: OrganizationId,
            _node_id: NodeId,
            _current_certificate_id: NodeCertificateId,
            _draft: NodeCertificateRotationDraft,
            _idempotency: IdempotencyRequest,
        ) -> Result<NodeCertificateRotationReservation, RepositoryError> {
            unreachable!("node access adapter never reserves certificate rotation")
        }

        async fn complete_certificate_rotation(
            &self,
            _completion: NodeCertificateRotationCompletion,
        ) -> Result<NodeCertificateRotationReservation, RepositoryError> {
            unreachable!("node access adapter never completes certificate rotation")
        }

        async fn authenticate_certificate(
            &self,
            _fingerprint: &str,
            _now: chrono::DateTime<Utc>,
        ) -> Result<Node, RepositoryError> {
            unreachable!("node access adapter never authenticates certificates")
        }

        async fn authenticate_rotation_certificate(
            &self,
            _fingerprint: &str,
            _now: chrono::DateTime<Utc>,
            _replay_not_before: chrono::DateTime<Utc>,
        ) -> Result<Node, RepositoryError> {
            unreachable!("node access adapter never authenticates rotation certificates")
        }

        async fn find_active_certificate(
            &self,
            _organization_id: OrganizationId,
            _node_id: NodeId,
        ) -> Result<NodeCertificate, RepositoryError> {
            unreachable!("node access adapter never loads active certificates")
        }

        async fn find_certificate(
            &self,
            _organization_id: OrganizationId,
            _node_id: NodeId,
            _certificate_id: NodeCertificateId,
        ) -> Result<NodeCertificate, RepositoryError> {
            unreachable!("node access adapter never loads certificates")
        }

        async fn record_heartbeat(
            &self,
            _update: NodeHeartbeatUpdate,
        ) -> Result<Node, RepositoryError> {
            unreachable!("node access adapter never records heartbeats")
        }

        async fn set_state(
            &self,
            _change: NodeStateChange,
        ) -> Result<IdempotentWrite<Node>, RepositoryError> {
            unreachable!("node access adapter never sets node state")
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _node_id: NodeId,
        ) -> Result<Node, RepositoryError> {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            self.node.clone().ok_or(RepositoryError::NotFound)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
        ) -> Result<Vec<Node>, RepositoryError> {
            unreachable!("node access adapter never lists nodes")
        }
    }

    #[tokio::test]
    async fn adapter_projects_only_exact_existing_node_evidence() {
        let organization_id = OrganizationId::new();
        let node_id = NodeId::new();
        let now = Utc::now();
        let node = Node {
            id: node_id,
            organization_id,
            name: NodeName::new("edge-1").expect("node name"),
            state: crate::modules::fleet::domain::value_objects::NodeState::Ready,
            agent_instance_id: Uuid::new_v4(),
            agent_version: "1.0.0".into(),
            capabilities: NodeCapabilities::new(
                "provider".to_owned(),
                "build".to_owned(),
                serde_json::json!({}),
            )
            .expect("capabilities"),
            enrolled_at: now,
            last_observed_at: now,
            last_sequence: 0,
            aggregate_version: 1,
        };
        let present = FleetEdgeNodeAccessAdapter::new(Arc::new(StubNodeRepository {
            node: Some(node),
            find_calls: AtomicUsize::new(0),
        }));
        let scope = EdgeNodeScope::new(organization_id, node_id).unwrap();
        assert!(present.node_exists(scope).await.unwrap());

        let missing = FleetEdgeNodeAccessAdapter::new(Arc::new(StubNodeRepository {
            node: None,
            find_calls: AtomicUsize::new(0),
        }));
        assert!(!missing.node_exists(scope).await.unwrap());
    }
}
