mod certificates;
mod control;
mod enrollment;
mod nodes;
mod queries;
mod rows;

use crate::modules::fleet::domain::entities::{EnrollmentToken, Node, NodeCertificate};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeCertificateRotationCompletion,
    NodeCertificateRotationDraft, NodeCertificateRotationReservation, NodeEnrollmentDraft,
    NodeEnrollmentReservation, NodeHeartbeatUpdate, NodeLogBatchReceiptDraft, NodeStateChange,
    RuntimeObservationRecord,
};
use crate::modules::fleet::domain::value_objects::EnrollmentTokenCredential;
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, IdempotencyRequest, IdempotentWrite, NodeCertificateId, NodeId,
    OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandLeaseResponse,
    NodeGatewayAck, NodeGatewayAckReceipt, NodeLogChunkReceipt, NodeObservationBatch,
    NodeObservationReceipt,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct PostgresNodeRepository {
    executor: PostgresExecutor,
}

impl PostgresNodeRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl INodeRepository for PostgresNodeRepository {
    async fn issue_enrollment_token(
        &self,
        token: EnrollmentToken,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<EnrollmentToken>, RepositoryError> {
        enrollment::issue_token(&self.executor, token, event, idempotency).await
    }

    async fn reserve_enrollment(
        &self,
        credential: &EnrollmentTokenCredential,
        draft: NodeEnrollmentDraft,
    ) -> Result<NodeEnrollmentReservation, RepositoryError> {
        enrollment::reserve(&self.executor, credential, draft).await
    }

    async fn complete_enrollment(
        &self,
        token_id: EnrollmentTokenId,
        node_id: NodeId,
        request_digest: &str,
        certificate: NodeCertificate,
        event: DomainEventEnvelope,
    ) -> Result<NodeEnrollmentReservation, RepositoryError> {
        enrollment::complete(
            &self.executor,
            token_id,
            node_id,
            request_digest,
            certificate,
            event,
        )
        .await
    }

    async fn reserve_certificate_rotation(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        current_certificate_id: NodeCertificateId,
        draft: NodeCertificateRotationDraft,
        idempotency: IdempotencyRequest,
    ) -> Result<NodeCertificateRotationReservation, RepositoryError> {
        certificates::reserve_rotation(
            &self.executor,
            organization_id,
            node_id,
            current_certificate_id,
            draft,
            idempotency,
        )
        .await
    }

    async fn complete_certificate_rotation(
        &self,
        completion: NodeCertificateRotationCompletion,
    ) -> Result<NodeCertificateRotationReservation, RepositoryError> {
        let NodeCertificateRotationCompletion {
            organization_id,
            node_id,
            current_certificate_id,
            replacement,
            rotated_at,
            event,
            idempotency,
        } = completion;
        certificates::complete_rotation(
            &self.executor,
            organization_id,
            node_id,
            current_certificate_id,
            replacement,
            rotated_at,
            event,
            idempotency,
        )
        .await
    }

    async fn authenticate_certificate(
        &self,
        fingerprint: &str,
        now: DateTime<Utc>,
    ) -> Result<Node, RepositoryError> {
        certificates::authenticate(&self.executor, fingerprint, now).await
    }

    async fn authenticate_rotation_certificate(
        &self,
        fingerprint: &str,
        now: DateTime<Utc>,
        replay_not_before: DateTime<Utc>,
    ) -> Result<Node, RepositoryError> {
        certificates::authenticate_rotation(&self.executor, fingerprint, now, replay_not_before)
            .await
    }

    async fn find_active_certificate(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<NodeCertificate, RepositoryError> {
        certificates::find_active(&self.executor, organization_id, node_id).await
    }

    async fn find_certificate(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        certificate_id: NodeCertificateId,
    ) -> Result<NodeCertificate, RepositoryError> {
        certificates::find(&self.executor, organization_id, node_id, certificate_id).await
    }

    async fn record_heartbeat(&self, update: NodeHeartbeatUpdate) -> Result<Node, RepositoryError> {
        nodes::record_heartbeat(&self.executor, update).await
    }

    async fn set_state(
        &self,
        change: NodeStateChange,
    ) -> Result<IdempotentWrite<Node>, RepositoryError> {
        let NodeStateChange {
            organization_id,
            node_id,
            state,
            expected_version,
            changed_at,
            event,
            idempotency,
        } = change;
        nodes::set_state(
            &self.executor,
            organization_id,
            node_id,
            state,
            expected_version,
            changed_at,
            event,
            idempotency,
        )
        .await
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<Node, RepositoryError> {
        nodes::find(&self.executor, organization_id, node_id).await
    }

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Node>, RepositoryError> {
        nodes::list(&self.executor, organization_id).await
    }
}

#[async_trait]
impl INodeControlRepository for PostgresNodeRepository {
    async fn enqueue_command(
        &self,
        draft: crate::modules::fleet::domain::entities::NodeCommandDraft,
    ) -> Result<
        IdempotentWrite<crate::modules::fleet::domain::entities::NodeCommand>,
        RepositoryError,
    > {
        control::enqueue(&self.executor, draft).await
    }

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: crate::modules::shared_kernel::domain::NodeCommandId,
    ) -> Result<Option<crate::modules::fleet::domain::entities::NodeCommand>, RepositoryError> {
        control::find_command(&self.executor, node_id, command_id).await
    }

    async fn lease_commands(
        &self,
        request: &NodeCommandLeaseRequest,
        lease_id: uuid::Uuid,
        now: DateTime<Utc>,
        leased_until: DateTime<Utc>,
    ) -> Result<NodeCommandLeaseResponse, RepositoryError> {
        control::lease(&self.executor, request, lease_id, now, leased_until).await
    }

    async fn acknowledge_command(
        &self,
        acknowledgement: NodeCommandAck,
        received_at: DateTime<Utc>,
    ) -> Result<IdempotentWrite<NodeCommandAck>, RepositoryError> {
        control::acknowledge(&self.executor, acknowledgement, received_at).await
    }

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: crate::modules::shared_kernel::domain::NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError> {
        control::command_acknowledgement(&self.executor, node_id, command_id).await
    }

    async fn record_observations(
        &self,
        batch: NodeObservationBatch,
        received_at: DateTime<Utc>,
    ) -> Result<NodeObservationReceipt, RepositoryError> {
        control::record_observations(&self.executor, batch, received_at).await
    }

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError> {
        control::latest_runtime_observation(&self.executor, node_id, unit_id, generation).await
    }

    async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<NodeGatewayAckReceipt, RepositoryError> {
        control::record_gateway_acknowledgement(&self.executor, acknowledgement, received_at).await
    }

    async fn record_log_chunks(
        &self,
        batch: NodeLogBatchReceiptDraft,
        received_at: DateTime<Utc>,
    ) -> Result<NodeLogChunkReceipt, RepositoryError> {
        control::record_log_chunks(&self.executor, batch, received_at).await
    }
}
