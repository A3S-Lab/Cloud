use crate::modules::fleet::domain::entities::{EnrollmentToken, Node, NodeCertificate};
use crate::modules::fleet::domain::repositories::{
    INodeRepository, NodeCertificateRotationCompletion, NodeCertificateRotationDraft,
    NodeCertificateRotationReservation, NodeEnrollmentDraft, NodeEnrollmentReservation,
    NodeHeartbeatUpdate, NodeStateChange,
};
use crate::modules::fleet::domain::value_objects::{EnrollmentTokenCredential, NodeState};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnrollmentTokenId, IdempotencyRequest, IdempotentWrite, NodeCertificateId,
    NodeCommandId, NodeId, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct State {
    tokens: BTreeMap<EnrollmentTokenId, EnrollmentToken>,
    token_by_digest: BTreeMap<String, EnrollmentTokenId>,
    token_idempotency: BTreeMap<(String, String), (String, EnrollmentTokenId)>,
    enrollment_request_digest: BTreeMap<EnrollmentTokenId, String>,
    enrollment_node: BTreeMap<EnrollmentTokenId, NodeId>,
    pub(super) nodes: BTreeMap<(OrganizationId, NodeId), Node>,
    certificates: BTreeMap<NodeCertificateId, NodeCertificate>,
    active_certificate_by_node: BTreeMap<NodeId, NodeCertificateId>,
    certificate_by_fingerprint: BTreeMap<String, NodeCertificateId>,
    rotation_idempotency: BTreeMap<(String, String), CertificateRotationRecord>,
    active_rotation_by_node: BTreeMap<NodeId, (String, String)>,
    state_idempotency: BTreeMap<(String, String), (String, Node)>,
    pub(super) commands: BTreeMap<NodeCommandId, super::in_memory_control::StoredNodeCommand>,
    pub(super) observations: BTreeMap<Uuid, super::in_memory_control::StoredObservation>,
    pub(super) gateway_acknowledgements:
        BTreeMap<Uuid, super::in_memory_control::StoredGatewayAcknowledgement>,
    pub(super) log_batches: BTreeMap<Uuid, super::in_memory_control::StoredLogBatch>,
    pub(super) log_chunks:
        BTreeMap<(NodeId, String, u64, u64), super::in_memory_control::StoredLogChunkReceipt>,
}

#[derive(Clone)]
struct CertificateRotationRecord {
    request_digest: String,
    organization_id: OrganizationId,
    node_id: NodeId,
    current_certificate_id: NodeCertificateId,
    replacement_certificate_id: NodeCertificateId,
    requested_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    replacement: Option<NodeCertificate>,
}

#[derive(Default)]
pub struct InMemoryNodeRepository {
    pub(super) state: RwLock<State>,
}

impl InMemoryNodeRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl INodeRepository for InMemoryNodeRepository {
    async fn issue_enrollment_token(
        &self,
        token: EnrollmentToken,
        _event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<EnrollmentToken>, RepositoryError> {
        let mut state = self.state.write().await;
        let key = (idempotency.scope.clone(), idempotency.key.clone());
        if let Some((digest, token_id)) = state.token_idempotency.get(&key) {
            if digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let existing = state.tokens.get(token_id).cloned().ok_or_else(|| {
                RepositoryError::Storage("token idempotency record is orphaned".into())
            })?;
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        if state
            .token_by_digest
            .contains_key(token.credential.digest())
        {
            return Err(RepositoryError::Conflict(
                "enrollment token secret already exists".into(),
            ));
        }
        if state.tokens.values().any(|existing| {
            existing.organization_id == token.organization_id && existing.name_key == token.name_key
        }) {
            return Err(RepositoryError::Conflict(
                "enrollment token name already exists".into(),
            ));
        }
        state
            .token_by_digest
            .insert(token.credential.digest().into(), token.id);
        state
            .token_idempotency
            .insert(key, (idempotency.request_digest.clone(), token.id));
        state.tokens.insert(token.id, token.clone());
        Ok(IdempotentWrite {
            value: token,
            replayed: false,
        })
    }

    async fn reserve_enrollment(
        &self,
        credential: &EnrollmentTokenCredential,
        mut draft: NodeEnrollmentDraft,
    ) -> Result<NodeEnrollmentReservation, RepositoryError> {
        draft.requested_at = canonical_timestamp("node enrollment request", draft.requested_at)
            .map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let token_id = state
            .token_by_digest
            .get(credential.digest())
            .copied()
            .ok_or(RepositoryError::NotFound)?;
        let existing_digest = state.enrollment_request_digest.get(&token_id).cloned();
        if let Some(existing_digest) = existing_digest {
            if existing_digest != draft.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return reservation(&state, token_id, true);
        }

        let token = state
            .tokens
            .get(&token_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("token digest index is orphaned".into()))?;
        if !token.is_usable_at(draft.requested_at) {
            return Err(RepositoryError::Conflict(
                "enrollment token is expired, revoked, or already used".into(),
            ));
        }
        if state.nodes.values().any(|node| {
            node.organization_id == token.organization_id
                && node.name.uniqueness_key() == draft.name.uniqueness_key()
        }) {
            return Err(RepositoryError::Conflict("node name already exists".into()));
        }
        let node = Node::enroll(
            draft.proposed_node_id,
            token.organization_id,
            draft.name,
            draft.agent_instance_id,
            draft.agent_version,
            draft.capabilities,
            draft.requested_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let stored_token = state
            .tokens
            .get_mut(&token_id)
            .ok_or_else(|| RepositoryError::Storage("enrollment token disappeared".into()))?;
        stored_token.used_at = Some(draft.requested_at);
        stored_token.aggregate_version += 1;
        state
            .enrollment_request_digest
            .insert(token_id, draft.request_digest);
        state.enrollment_node.insert(token_id, node.id);
        state
            .nodes
            .insert((node.organization_id, node.id), node.clone());
        reservation(&state, token_id, false)
    }

    async fn complete_enrollment(
        &self,
        token_id: EnrollmentTokenId,
        node_id: NodeId,
        request_digest: &str,
        certificate: NodeCertificate,
        _event: DomainEventEnvelope,
    ) -> Result<NodeEnrollmentReservation, RepositoryError> {
        let mut state = self.state.write().await;
        if state
            .enrollment_request_digest
            .get(&token_id)
            .map(String::as_str)
            != Some(request_digest)
            || state.enrollment_node.get(&token_id) != Some(&node_id)
            || certificate.node_id != node_id
        {
            return Err(RepositoryError::IdempotencyConflict);
        }
        if state.active_certificate_by_node.contains_key(&node_id) {
            return reservation(&state, token_id, true);
        }
        if state
            .certificate_by_fingerprint
            .contains_key(&certificate.fingerprint)
        {
            return Err(RepositoryError::Conflict(
                "certificate fingerprint already exists".into(),
            ));
        }
        state
            .certificate_by_fingerprint
            .insert(certificate.fingerprint.clone(), certificate.id);
        state
            .active_certificate_by_node
            .insert(node_id, certificate.id);
        state.certificates.insert(certificate.id, certificate);
        reservation(&state, token_id, false)
    }

    async fn reserve_certificate_rotation(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        current_certificate_id: NodeCertificateId,
        mut draft: NodeCertificateRotationDraft,
        idempotency: IdempotencyRequest,
    ) -> Result<NodeCertificateRotationReservation, RepositoryError> {
        draft.requested_at =
            canonical_timestamp("node certificate rotation request", draft.requested_at)
                .map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let idempotency_key = (idempotency.scope.clone(), idempotency.key.clone());
        if let Some(record) = state.rotation_idempotency.get(&idempotency_key) {
            if record.request_digest != idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return rotation_reservation(&state, record, true);
        }
        let node = state
            .nodes
            .get(&(organization_id, node_id))
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if node.state == NodeState::Revoked
            || state.active_certificate_by_node.get(&node_id) != Some(&current_certificate_id)
        {
            return Err(RepositoryError::Conflict(
                "certificate rotation identity is invalid".into(),
            ));
        }
        if state.active_rotation_by_node.contains_key(&node_id) {
            return Err(RepositoryError::Conflict(
                "another certificate rotation is already pending".into(),
            ));
        }
        if !state.certificates.contains_key(&current_certificate_id) {
            return Err(RepositoryError::Storage(
                "active certificate is missing".into(),
            ));
        }
        let record = CertificateRotationRecord {
            request_digest: idempotency.request_digest,
            organization_id,
            node_id,
            current_certificate_id,
            replacement_certificate_id: draft.replacement_certificate_id,
            requested_at: draft.requested_at,
            completed_at: None,
            replacement: None,
        };
        state
            .active_rotation_by_node
            .insert(node_id, idempotency_key.clone());
        state
            .rotation_idempotency
            .insert(idempotency_key, record.clone());
        rotation_reservation(&state, &record, false)
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
            event: _,
            idempotency,
        } = completion;
        let rotated_at = canonical_timestamp("node certificate rotation", rotated_at)
            .map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let idempotency_key = (idempotency.scope.clone(), idempotency.key.clone());
        let record = state
            .rotation_idempotency
            .get(&idempotency_key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if record.request_digest != idempotency.request_digest
            || record.organization_id != organization_id
            || record.node_id != node_id
            || record.current_certificate_id != current_certificate_id
            || record.replacement_certificate_id != replacement.id
            || replacement.node_id != node_id
        {
            return Err(RepositoryError::IdempotencyConflict);
        }
        if record.replacement.is_some() {
            return rotation_reservation(&state, &record, true);
        }
        if state.active_certificate_by_node.get(&node_id) != Some(&current_certificate_id)
            || state.active_rotation_by_node.get(&node_id) != Some(&idempotency_key)
        {
            return Err(RepositoryError::Conflict(
                "certificate rotation reservation is no longer active".into(),
            ));
        }
        if state
            .certificate_by_fingerprint
            .contains_key(&replacement.fingerprint)
        {
            return Err(RepositoryError::Conflict(
                "certificate fingerprint already exists".into(),
            ));
        }
        state
            .certificates
            .get_mut(&current_certificate_id)
            .ok_or_else(|| RepositoryError::Storage("active certificate is missing".into()))?
            .revoked_at = Some(rotated_at);
        state
            .certificate_by_fingerprint
            .insert(replacement.fingerprint.clone(), replacement.id);
        state
            .active_certificate_by_node
            .insert(node_id, replacement.id);
        state
            .certificates
            .insert(replacement.id, replacement.clone());
        let node = state
            .nodes
            .get_mut(&(organization_id, node_id))
            .ok_or_else(|| RepositoryError::Storage("rotation node disappeared".into()))?;
        node.aggregate_version += 1;
        state.active_rotation_by_node.remove(&node_id);
        let stored_record = state
            .rotation_idempotency
            .get_mut(&idempotency_key)
            .ok_or_else(|| RepositoryError::Storage("rotation reservation disappeared".into()))?;
        stored_record.replacement = Some(replacement);
        stored_record.completed_at = Some(rotated_at);
        let completed = stored_record.clone();
        rotation_reservation(&state, &completed, false)
    }

    async fn authenticate_certificate(
        &self,
        fingerprint: &str,
        now: DateTime<Utc>,
    ) -> Result<Node, RepositoryError> {
        let state = self.state.read().await;
        let certificate_id = state
            .certificate_by_fingerprint
            .get(fingerprint)
            .ok_or(RepositoryError::NotFound)?;
        let certificate = state
            .certificates
            .get(certificate_id)
            .ok_or_else(|| RepositoryError::Storage("certificate index is orphaned".into()))?;
        if !certificate.is_valid_at(now)
            || state.active_certificate_by_node.get(&certificate.node_id) != Some(certificate_id)
        {
            return Err(RepositoryError::NotFound);
        }
        let node = state
            .nodes
            .values()
            .find(|node| node.id == certificate.node_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("certificate node is missing".into()))?;
        if node.state == NodeState::Revoked {
            return Err(RepositoryError::NotFound);
        }
        Ok(node)
    }

    async fn authenticate_rotation_certificate(
        &self,
        fingerprint: &str,
        now: DateTime<Utc>,
        replay_not_before: DateTime<Utc>,
    ) -> Result<Node, RepositoryError> {
        if replay_not_before > now {
            return Err(RepositoryError::Conflict(
                "certificate rotation replay window is invalid".into(),
            ));
        }
        let state = self.state.read().await;
        let certificate_id = state
            .certificate_by_fingerprint
            .get(fingerprint)
            .ok_or(RepositoryError::NotFound)?;
        let certificate = state
            .certificates
            .get(certificate_id)
            .ok_or_else(|| RepositoryError::Storage("certificate index is orphaned".into()))?;
        let active = certificate.is_valid_at(now)
            && state.active_certificate_by_node.get(&certificate.node_id) == Some(certificate_id);
        let replayable = certificate.issued_at <= now
            && certificate.expires_at > now
            && state.rotation_idempotency.values().any(|rotation| {
                rotation.current_certificate_id == *certificate_id
                    && rotation.completed_at.is_some_and(|completed_at| {
                        completed_at >= replay_not_before && completed_at <= now
                    })
                    && rotation.replacement.as_ref().is_some_and(|replacement| {
                        state.active_certificate_by_node.get(&certificate.node_id)
                            == Some(&replacement.id)
                            && replacement.is_valid_at(now)
                    })
            });
        if !active && !replayable {
            return Err(RepositoryError::NotFound);
        }
        let node = state
            .nodes
            .values()
            .find(|node| node.id == certificate.node_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("certificate node is missing".into()))?;
        if node.state == NodeState::Revoked {
            return Err(RepositoryError::NotFound);
        }
        Ok(node)
    }

    async fn find_active_certificate(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<NodeCertificate, RepositoryError> {
        let state = self.state.read().await;
        if !state.nodes.contains_key(&(organization_id, node_id)) {
            return Err(RepositoryError::NotFound);
        }
        let certificate_id = state
            .active_certificate_by_node
            .get(&node_id)
            .ok_or(RepositoryError::NotFound)?;
        state
            .certificates
            .get(certificate_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("active certificate is missing".into()))
    }

    async fn find_certificate(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        certificate_id: NodeCertificateId,
    ) -> Result<NodeCertificate, RepositoryError> {
        let state = self.state.read().await;
        if !state.nodes.contains_key(&(organization_id, node_id)) {
            return Err(RepositoryError::NotFound);
        }
        state
            .certificates
            .get(&certificate_id)
            .filter(|certificate| certificate.node_id == node_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn record_heartbeat(
        &self,
        mut update: NodeHeartbeatUpdate,
    ) -> Result<Node, RepositoryError> {
        update.observed_at = canonical_timestamp("node heartbeat", update.observed_at)
            .map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let node_key = state
            .nodes
            .iter()
            .find(|(_, node)| node.id == update.node_id)
            .map(|(key, _)| *key)
            .ok_or(RepositoryError::NotFound)?;
        let projected = project_heartbeat(
            state
                .nodes
                .get(&node_key)
                .ok_or_else(|| RepositoryError::Storage("heartbeat node disappeared".into()))?,
            &update,
        )?;
        state.nodes.insert(node_key, projected.clone());
        Ok(projected)
    }

    async fn set_state(
        &self,
        change: NodeStateChange,
    ) -> Result<IdempotentWrite<Node>, RepositoryError> {
        let NodeStateChange {
            organization_id,
            node_id,
            state: requested_state,
            expected_version,
            changed_at,
            event: _,
            idempotency,
        } = change;
        let changed_at = canonical_timestamp("node state change", changed_at)
            .map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let idempotency_key = (idempotency.scope.clone(), idempotency.key.clone());
        if let Some((request_digest, node)) = state.state_idempotency.get(&idempotency_key) {
            if request_digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: node.clone(),
                replayed: true,
            });
        }
        let node = state
            .nodes
            .get_mut(&(organization_id, node_id))
            .ok_or(RepositoryError::NotFound)?;
        if node.aggregate_version != expected_version {
            return Err(RepositoryError::Conflict(
                "node aggregate version changed".into(),
            ));
        }
        match requested_state {
            NodeState::Ready => node.mark_ready(),
            NodeState::Draining => node.drain(),
            NodeState::Revoked => {
                node.revoke();
                Ok(())
            }
            NodeState::Pending => Err("node cannot transition back to pending".into()),
        }
        .map_err(RepositoryError::Conflict)?;
        let result = node.clone();
        if requested_state == NodeState::Revoked {
            if let Some(certificate_id) = state.active_certificate_by_node.remove(&node_id) {
                if let Some(certificate) = state.certificates.get_mut(&certificate_id) {
                    certificate.revoked_at = Some(changed_at);
                }
            }
        }
        state.state_idempotency.insert(
            idempotency_key,
            (idempotency.request_digest, result.clone()),
        );
        Ok(IdempotentWrite {
            value: result,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<Node, RepositoryError> {
        self.state
            .read()
            .await
            .nodes
            .get(&(organization_id, node_id))
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Node>, RepositoryError> {
        let mut nodes = self
            .state
            .read()
            .await
            .nodes
            .values()
            .filter(|node| node.organization_id == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        nodes.sort_by_key(|node| (node.name.uniqueness_key().to_owned(), node.id));
        Ok(nodes)
    }
}

pub(super) fn project_heartbeat(
    current: &Node,
    update: &NodeHeartbeatUpdate,
) -> Result<Node, RepositoryError> {
    let mut node = current.clone();
    if node.state == NodeState::Revoked {
        return Err(RepositoryError::NotFound);
    }
    if update.observed_at < node.last_observed_at {
        return Err(RepositoryError::Conflict(
            "node heartbeat moved backwards".into(),
        ));
    }
    if update.observed_at == node.last_observed_at {
        if node.agent_instance_id != update.agent_instance_id
            || node.agent_version != update.agent_version
            || node.capabilities != update.capabilities
        {
            return Err(RepositoryError::Conflict(
                "node heartbeat timestamp was reused with different content".into(),
            ));
        }
        if node.state != NodeState::Pending {
            return Ok(node);
        }
    }
    node.agent_instance_id = update.agent_instance_id;
    node.agent_version = update.agent_version.clone();
    node.capabilities = update.capabilities.clone();
    node.last_observed_at = update.observed_at;
    if node.state == NodeState::Pending {
        node.mark_ready().map_err(RepositoryError::Conflict)?;
    } else {
        node.aggregate_version += 1;
    }
    Ok(node)
}

fn reservation(
    state: &State,
    token_id: EnrollmentTokenId,
    replayed: bool,
) -> Result<NodeEnrollmentReservation, RepositoryError> {
    let enrollment_token = state
        .tokens
        .get(&token_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("enrollment token is missing".into()))?;
    let node_id = state
        .enrollment_node
        .get(&token_id)
        .copied()
        .ok_or_else(|| RepositoryError::Storage("enrollment node binding is missing".into()))?;
    let node = state
        .nodes
        .values()
        .find(|node| node.id == node_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("enrollment node is missing".into()))?;
    let certificate = state
        .active_certificate_by_node
        .get(&node_id)
        .and_then(|certificate_id| state.certificates.get(certificate_id))
        .cloned();
    Ok(NodeEnrollmentReservation {
        enrollment_token,
        node,
        certificate,
        replayed,
    })
}

fn rotation_reservation(
    state: &State,
    record: &CertificateRotationRecord,
    replayed: bool,
) -> Result<NodeCertificateRotationReservation, RepositoryError> {
    let node = state
        .nodes
        .get(&(record.organization_id, record.node_id))
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("rotation node is missing".into()))?;
    let current_certificate = state
        .certificates
        .get(&record.current_certificate_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("rotation certificate is missing".into()))?;
    Ok(NodeCertificateRotationReservation {
        node,
        current_certificate,
        replacement_certificate_id: record.replacement_certificate_id,
        requested_at: record.requested_at,
        replacement: record.replacement.clone(),
        replayed,
    })
}
