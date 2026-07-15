use super::rows::{
    self, CertificateRow, EnrollmentTokenRow, NodeRow, SELECT_CERTIFICATES, SELECT_NODES,
    SELECT_TOKENS,
};
use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::fleet::domain::entities::{EnrollmentToken, Node, NodeCertificate};
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, NodeCertificateId, NodeId, OrganizationId,
};
use a3s_orm::{sql_query, PostgresTransaction};

pub(super) async fn token_by_id(
    transaction: &PostgresTransaction,
    token_id: EnrollmentTokenId,
    lock: bool,
) -> Result<Option<EnrollmentToken>, PostgresPersistenceError> {
    let mut query = sql_query::<EnrollmentTokenRow>(SELECT_TOKENS)
        .append(" where id = ")
        .bind(token_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::token)
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn node_by_identity(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    node_id: NodeId,
    lock: bool,
) -> Result<Option<Node>, PostgresPersistenceError> {
    let mut query = sql_query::<NodeRow>(SELECT_NODES)
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and id = ")
        .bind(node_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::node)
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn node_by_id(
    transaction: &PostgresTransaction,
    node_id: NodeId,
    lock: bool,
) -> Result<Option<Node>, PostgresPersistenceError> {
    let mut query = sql_query::<NodeRow>(SELECT_NODES)
        .append(" where id = ")
        .bind(node_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::node)
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn certificate_by_id(
    transaction: &PostgresTransaction,
    certificate_id: NodeCertificateId,
    lock: bool,
) -> Result<Option<NodeCertificate>, PostgresPersistenceError> {
    let mut query = sql_query::<CertificateRow>(SELECT_CERTIFICATES)
        .append(" where id = ")
        .bind(certificate_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::certificate)
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn active_certificate_by_node(
    transaction: &PostgresTransaction,
    node_id: NodeId,
    lock: bool,
) -> Result<Option<NodeCertificate>, PostgresPersistenceError> {
    let mut query = sql_query::<CertificateRow>(SELECT_CERTIFICATES)
        .append(" where node_id = ")
        .bind(node_id.as_uuid())
        .append(" and revoked_at is null");
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::certificate)
        .transpose()
        .map_err(Into::into)
}
