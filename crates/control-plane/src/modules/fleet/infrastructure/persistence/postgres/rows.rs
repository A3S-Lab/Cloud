use crate::modules::fleet::domain::entities::{EnrollmentToken, Node, NodeCertificate};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName, NodeState,
};
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, NodeCertificateId, NodeId, OrganizationId, RepositoryError,
};
use a3s_orm::{DecodeError, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

pub(super) struct EnrollmentTokenRow {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    token_hash: String,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    used_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

pub(super) struct NodeRow {
    organization_id: Uuid,
    id: Uuid,
    name: String,
    state: String,
    agent_instance_id: Uuid,
    agent_version: String,
    provider_id: String,
    provider_build: String,
    capabilities_digest: String,
    capabilities_document: Value,
    enrolled_at: DateTime<Utc>,
    last_observed_at: DateTime<Utc>,
    last_sequence: u64,
    aggregate_version: u64,
}

pub(super) struct CertificateRow {
    id: Uuid,
    node_id: Uuid,
    serial_number: String,
    fingerprint: String,
    certificate_pem: String,
    ca_bundle_pem: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for EnrollmentTokenRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            name: decode(row, 2)?,
            token_hash: decode(row, 3)?,
            aggregate_version: decode(row, 4)?,
            created_at: decode(row, 5)?,
            expires_at: decode(row, 6)?,
            used_at: decode(row, 7)?,
            revoked_at: decode(row, 8)?,
        })
    }
}

impl FromRow for NodeRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            name: decode(row, 2)?,
            state: decode(row, 3)?,
            agent_instance_id: decode(row, 4)?,
            agent_version: decode(row, 5)?,
            provider_id: decode(row, 6)?,
            provider_build: decode(row, 7)?,
            capabilities_digest: decode(row, 8)?,
            capabilities_document: decode(row, 9)?,
            enrolled_at: decode(row, 10)?,
            last_observed_at: decode(row, 11)?,
            last_sequence: decode(row, 12)?,
            aggregate_version: decode(row, 13)?,
        })
    }
}

impl FromRow for CertificateRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            node_id: decode(row, 1)?,
            serial_number: decode(row, 2)?,
            fingerprint: decode(row, 3)?,
            certificate_pem: decode(row, 4)?,
            ca_bundle_pem: decode(row, 5)?,
            issued_at: decode(row, 6)?,
            expires_at: decode(row, 7)?,
            revoked_at: decode(row, 8)?,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}

pub(super) const SELECT_TOKENS: &str = "select id, organization_id, name, token_hash, aggregate_version, created_at, expires_at, used_at, revoked_at from enrollment_tokens";
pub(super) const SELECT_NODES: &str = "select organization_id, id, name, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, last_sequence, aggregate_version from nodes";
pub(super) const SELECT_CERTIFICATES: &str = "select id, node_id, serial_number, fingerprint, certificate_pem, ca_bundle_pem, issued_at, expires_at, revoked_at from node_certificates";

pub(super) fn token(row: EnrollmentTokenRow) -> Result<EnrollmentToken, RepositoryError> {
    let EnrollmentTokenRow {
        id,
        organization_id,
        name,
        token_hash,
        aggregate_version,
        created_at,
        expires_at,
        used_at,
        revoked_at,
    } = row;
    let credential = EnrollmentTokenCredential::from_digest(token_hash).map_err(|error| {
        RepositoryError::Storage(format!(
            "stored enrollment token digest is invalid: {error}"
        ))
    })?;
    let mut value = EnrollmentToken::new(
        EnrollmentTokenId::from_uuid(id),
        OrganizationId::from_uuid(organization_id),
        name,
        credential,
        created_at,
        expires_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!("stored enrollment token is invalid: {error}"))
    })?;
    value.used_at = used_at;
    value.revoked_at = revoked_at;
    value.aggregate_version = aggregate_version;
    Ok(value)
}

pub(super) fn node(row: NodeRow) -> Result<Node, RepositoryError> {
    let NodeRow {
        organization_id,
        id,
        name,
        state,
        agent_instance_id,
        agent_version,
        provider_id,
        provider_build,
        capabilities_digest,
        capabilities_document,
        enrolled_at,
        last_observed_at,
        last_sequence,
        aggregate_version,
    } = row;
    let name = NodeName::new(name).map_err(|error| {
        RepositoryError::Storage(format!("stored node name is invalid: {error}"))
    })?;
    let state = NodeState::parse(&state).map_err(|error| {
        RepositoryError::Storage(format!("stored node state is invalid: {error}"))
    })?;
    let capabilities = NodeCapabilities::new(provider_id, provider_build, capabilities_document)
        .map_err(|error| {
            RepositoryError::Storage(format!("stored node capabilities are invalid: {error}"))
        })?;
    if capabilities.digest() != capabilities_digest {
        return Err(RepositoryError::Storage(
            "stored node capabilities digest does not match its document".into(),
        ));
    }
    Ok(Node {
        id: NodeId::from_uuid(id),
        organization_id: OrganizationId::from_uuid(organization_id),
        name,
        state,
        agent_instance_id,
        agent_version,
        capabilities,
        enrolled_at,
        last_observed_at,
        last_sequence,
        aggregate_version,
    })
}

pub(super) fn certificate(row: CertificateRow) -> Result<NodeCertificate, RepositoryError> {
    let CertificateRow {
        id,
        node_id,
        serial_number,
        fingerprint,
        certificate_pem,
        ca_bundle_pem,
        issued_at,
        expires_at,
        revoked_at,
    } = row;
    let mut certificate = NodeCertificate::new(
        NodeCertificateId::from_uuid(id),
        NodeId::from_uuid(node_id),
        crate::modules::fleet::domain::entities::NodeCertificateMaterial {
            serial_number,
            fingerprint,
            certificate_pem,
            ca_bundle_pem,
            issued_at,
            expires_at,
        },
    )
    .map_err(|error| {
        RepositoryError::Storage(format!("stored node certificate is invalid: {error}"))
    })?;
    certificate.revoked_at = revoked_at;
    Ok(certificate)
}
