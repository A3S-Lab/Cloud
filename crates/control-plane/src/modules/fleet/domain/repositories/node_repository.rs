use crate::modules::fleet::domain::entities::{EnrollmentToken, Node, NodeCertificate};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName, NodeState,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NodeCertificateId, NodeId, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NodeEnrollmentDraft {
    pub proposed_node_id: NodeId,
    pub name: NodeName,
    pub agent_instance_id: Uuid,
    pub agent_version: String,
    pub capabilities: NodeCapabilities,
    pub request_digest: String,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeEnrollmentReservation {
    pub enrollment_token: EnrollmentToken,
    pub node: Node,
    pub certificate: Option<NodeCertificate>,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct NodeHeartbeatUpdate {
    pub node_id: NodeId,
    pub agent_instance_id: Uuid,
    pub agent_version: String,
    pub capabilities: NodeCapabilities,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NodeCertificateRotationDraft {
    pub replacement_certificate_id: NodeCertificateId,
    pub requested_at: DateTime<Utc>,
}

pub struct NodeCertificateRotationCompletion {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub current_certificate_id: NodeCertificateId,
    pub replacement: NodeCertificate,
    pub rotated_at: DateTime<Utc>,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

pub struct NodeStateChange {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub state: NodeState,
    pub expected_version: u64,
    pub changed_at: DateTime<Utc>,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCertificateRotationReservation {
    pub node: Node,
    pub current_certificate: NodeCertificate,
    pub replacement_certificate_id: NodeCertificateId,
    pub requested_at: DateTime<Utc>,
    pub replacement: Option<NodeCertificate>,
    pub replayed: bool,
}

#[async_trait]
pub trait INodeRepository: Send + Sync {
    async fn issue_enrollment_token(
        &self,
        token: EnrollmentToken,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<EnrollmentToken>, RepositoryError>;

    async fn reserve_enrollment(
        &self,
        credential: &EnrollmentTokenCredential,
        draft: NodeEnrollmentDraft,
    ) -> Result<NodeEnrollmentReservation, RepositoryError>;

    async fn complete_enrollment(
        &self,
        token_id: crate::modules::shared_kernel::domain::EnrollmentTokenId,
        node_id: NodeId,
        request_digest: &str,
        certificate: NodeCertificate,
        event: DomainEventEnvelope,
    ) -> Result<NodeEnrollmentReservation, RepositoryError>;

    async fn reserve_certificate_rotation(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        current_certificate_id: NodeCertificateId,
        draft: NodeCertificateRotationDraft,
        idempotency: IdempotencyRequest,
    ) -> Result<NodeCertificateRotationReservation, RepositoryError>;

    async fn complete_certificate_rotation(
        &self,
        completion: NodeCertificateRotationCompletion,
    ) -> Result<NodeCertificateRotationReservation, RepositoryError>;

    async fn authenticate_certificate(
        &self,
        fingerprint: &str,
        now: DateTime<Utc>,
    ) -> Result<Node, RepositoryError>;

    async fn authenticate_rotation_certificate(
        &self,
        fingerprint: &str,
        now: DateTime<Utc>,
        replay_not_before: DateTime<Utc>,
    ) -> Result<Node, RepositoryError>;

    async fn find_active_certificate(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<NodeCertificate, RepositoryError>;

    async fn find_certificate(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        certificate_id: NodeCertificateId,
    ) -> Result<NodeCertificate, RepositoryError>;

    async fn record_heartbeat(&self, update: NodeHeartbeatUpdate) -> Result<Node, RepositoryError>;

    async fn set_state(
        &self,
        change: NodeStateChange,
    ) -> Result<IdempotentWrite<Node>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<Node, RepositoryError>;

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Node>, RepositoryError>;
}
