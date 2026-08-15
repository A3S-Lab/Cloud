use crate::modules::connectors::domain::{
    ConnectorExecutionEvidence, ConnectorExecutionEvidenceCursor, ConnectorExecutionOutcome,
    IConnectorExecutionEvidenceRepository, MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) const SELECT_EVIDENCE: &str = "select organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, outcome, response_status, response_digest, response_body_bytes, retry_after_seconds, started_at, completed_at from connector_execution_evidence";

#[derive(Clone)]
pub struct PostgresConnectorExecutionEvidenceRepository {
    executor: PostgresExecutor,
}

impl PostgresConnectorExecutionEvidenceRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IConnectorExecutionEvidenceRepository for PostgresConnectorExecutionEvidenceRepository {
    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionEvidence>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                evidence_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and profile_id = ")
                    .bind(profile_id.as_uuid())
                    .append(" and revision_id = ")
                    .bind(revision_id.as_uuid())
                    .append(" and attempt_id = ")
                    .bind(attempt_id),
            )
            .await
            .map_err(storage)?
            .map(decode_evidence)
            .transpose()
    }

    async fn list_page(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        after: Option<ConnectorExecutionEvidenceCursor>,
        limit: usize,
    ) -> Result<Vec<ConnectorExecutionEvidence>, RepositoryError> {
        let after = after
            .map(ConnectorExecutionEvidenceCursor::validate)
            .transpose()
            .map_err(RepositoryError::Storage)?;
        let mut query = evidence_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(environment_id.as_uuid())
            .append(" and profile_id = ")
            .bind(profile_id.as_uuid())
            .append(" and revision_id = ")
            .bind(revision_id.as_uuid());
        if let Some(cursor) = after {
            query = query
                .append(" and (completed_at < ")
                .bind(cursor.completed_at)
                .append(" or (completed_at = ")
                .bind(cursor.completed_at)
                .append(" and attempt_id < ")
                .bind(cursor.attempt_id)
                .append("))");
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .append(" order by completed_at desc, attempt_id desc limit ")
                    .bind(limit.clamp(1, MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE + 1)),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_evidence)
            .collect()
    }
}

pub(super) struct ConnectorExecutionEvidenceRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    revision_id: Uuid,
    attempt_id: Uuid,
    request_digest: String,
    request_body_bytes: u64,
    outcome: String,
    response_status: Option<i32>,
    response_digest: Option<String>,
    response_body_bytes: Option<u64>,
    retry_after_seconds: Option<u64>,
    started_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
}

impl FromRow for ConnectorExecutionEvidenceRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            profile_id: decode(row, 3)?,
            revision_id: decode(row, 4)?,
            attempt_id: decode(row, 5)?,
            request_digest: decode(row, 6)?,
            request_body_bytes: decode(row, 7)?,
            outcome: decode(row, 8)?,
            response_status: decode(row, 9)?,
            response_digest: decode(row, 10)?,
            response_body_bytes: decode(row, 11)?,
            retry_after_seconds: decode(row, 12)?,
            started_at: decode(row, 13)?,
            completed_at: decode(row, 14)?,
        })
    }
}

pub(super) fn evidence_select() -> a3s_orm::SqlQuery<ConnectorExecutionEvidenceRow> {
    sql_query::<ConnectorExecutionEvidenceRow>(SELECT_EVIDENCE)
}

pub(super) fn decode_evidence(
    row: ConnectorExecutionEvidenceRow,
) -> Result<ConnectorExecutionEvidence, RepositoryError> {
    let response_status = row
        .response_status
        .map(u16::try_from)
        .transpose()
        .map_err(|error| {
            storage(format!(
                "stored Connector response status is invalid: {error}"
            ))
        })?;
    ConnectorExecutionEvidence::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        EnvironmentId::from_uuid(row.environment_id),
        ConnectorProfileId::from_uuid(row.profile_id),
        ConnectorRevisionId::from_uuid(row.revision_id),
        row.attempt_id,
        Sha256Digest::parse(row.request_digest).map_err(|error| {
            storage(format!(
                "stored Connector request digest is invalid: {error}"
            ))
        })?,
        row.request_body_bytes,
        ConnectorExecutionOutcome::parse(&row.outcome)
            .map_err(|error| storage(format!("stored Connector outcome is invalid: {error}")))?,
        response_status,
        row.response_digest
            .map(Sha256Digest::parse)
            .transpose()
            .map_err(|error| {
                storage(format!(
                    "stored Connector response digest is invalid: {error}"
                ))
            })?,
        row.response_body_bytes,
        row.retry_after_seconds,
        row.started_at,
        row.completed_at,
    )
    .map_err(|error| {
        storage(format!(
            "stored Connector execution evidence is invalid: {error}"
        ))
    })
}

pub(super) fn insert_evidence_query(
    evidence: &ConnectorExecutionEvidence,
) -> a3s_orm::SqlQuery<()> {
    sql_query::<()>("insert into connector_execution_evidence (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, outcome, response_status, response_digest, response_body_bytes, retry_after_seconds, started_at, completed_at) values (")
        .bind(evidence.organization_id().as_uuid())
        .append(", ")
        .bind(evidence.project_id().as_uuid())
        .append(", ")
        .bind(evidence.environment_id().as_uuid())
        .append(", ")
        .bind(evidence.profile_id().as_uuid())
        .append(", ")
        .bind(evidence.revision_id().as_uuid())
        .append(", ")
        .bind(evidence.attempt_id())
        .append(", ")
        .bind(evidence.request_digest().as_str())
        .append(", ")
        .bind(evidence.request_body_bytes())
        .append(", ")
        .bind(evidence.outcome().as_str())
        .append(", ")
        .bind(evidence.response_status().map(i32::from))
        .append(", ")
        .bind(evidence.response_digest().map(Sha256Digest::as_str))
        .append(", ")
        .bind(evidence.response_body_bytes())
        .append(", ")
        .bind(evidence.retry_after().map(|value| value.as_secs()))
        .append(", ")
        .bind(evidence.started_at())
        .append(", ")
        .bind(evidence.completed_at())
        .append(")")
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
