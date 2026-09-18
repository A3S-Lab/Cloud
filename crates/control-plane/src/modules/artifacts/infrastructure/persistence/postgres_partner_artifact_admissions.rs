use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_unique_violation, store_idempotency,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::artifacts::domain::entities::{
    PartnerArtifactAdmission, PartnerArtifactAdmissionId, PartnerArtifactKind,
};
use crate::modules::artifacts::domain::repositories::{
    AdmitPartnerArtifactWrite, IPartnerArtifactAdmissionRepository,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, RepositoryError, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresPartnerArtifactAdmissionRepository {
    executor: PostgresExecutor,
}

impl PostgresPartnerArtifactAdmissionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

struct AdmissionRow {
    id: Uuid,
    organization_id: Uuid,
    content_digest: String,
    kind: String,
    byte_size: i64,
    partner_ref: String,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

impl FromRow for AdmissionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            content_digest: decode(row, 2)?,
            kind: decode(row, 3)?,
            byte_size: decode(row, 4)?,
            partner_ref: decode(row, 5)?,
            aggregate_version: decode(row, 6)?,
            created_at: decode(row, 7)?,
        })
    }
}

const SELECT_ADMISSIONS: &str = "select id, organization_id, content_digest, kind, byte_size, partner_ref, aggregate_version, created_at from partner_artifact_admissions";

fn map_row(row: AdmissionRow) -> Result<PartnerArtifactAdmission, RepositoryError> {
    let kind = PartnerArtifactKind::parse(&row.kind).map_err(RepositoryError::Storage)?;
    let content_digest =
        Sha256Digest::parse(row.content_digest).map_err(RepositoryError::Storage)?;
    let byte_size = u64::try_from(row.byte_size).map_err(|error| {
        RepositoryError::Storage(format!("stored partner artifact size is invalid: {error}"))
    })?;
    let mut admission = PartnerArtifactAdmission::create(
        PartnerArtifactAdmissionId::from_uuid(row.id),
        OrganizationId::from_uuid(row.organization_id),
        content_digest,
        kind,
        byte_size,
        row.partner_ref,
        row.created_at,
    )
    .map_err(RepositoryError::Storage)?;
    admission.aggregate_version = row.aggregate_version;
    Ok(admission)
}

#[async_trait]
impl IPartnerArtifactAdmissionRepository for PostgresPartnerArtifactAdmissionRepository {
    async fn admit(
        &self,
        write: AdmitPartnerArtifactWrite,
    ) -> Result<IdempotentWrite<PartnerArtifactAdmission>, RepositoryError> {
        let byte_size = i64::try_from(write.admission.byte_size).map_err(|_| {
            RepositoryError::Storage("partner artifact byte size exceeds signed storage".into())
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<PartnerArtifactAdmission>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let existing = fetch_optional::<AdmissionRow, _>(
                        transaction,
                        sql_query::<AdmissionRow>(SELECT_ADMISSIONS)
                            .append(" where organization_id = ")
                            .bind(write.admission.organization_id.as_uuid())
                            .append(" and content_digest = ")
                            .bind(write.admission.content_digest.as_str()),
                    )
                    .await?;
                    if let Some(existing) = existing {
                        let existing = map_row(existing).map_err(PostgresPersistenceError::from)?;
                        if !existing.same_facts(&write.admission) {
                            return Err(PostgresPersistenceError::from(RepositoryError::Conflict(
                                "partner artifact digest is already admitted with different metadata"
                                    .into(),
                            )));
                        }
                        store_idempotency(transaction, &write.idempotency, &existing).await?;
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    match execute(
                        transaction,
                        sql_query::<()>(
                            "insert into partner_artifact_admissions (id, organization_id, content_digest, kind, byte_size, partner_ref, aggregate_version, created_at) values (",
                        )
                        .bind(write.admission.id.as_uuid())
                        .append(", ")
                        .bind(write.admission.organization_id.as_uuid())
                        .append(", ")
                        .bind(write.admission.content_digest.as_str())
                        .append(", ")
                        .bind(write.admission.kind.as_str())
                        .append(", ")
                        .bind(byte_size)
                        .append(", ")
                        .bind(write.admission.partner_ref.as_str())
                        .append(", ")
                        .bind(write.admission.aggregate_version)
                        .append(", ")
                        .bind(write.admission.created_at)
                        .append(")"),
                    )
                    .await
                    {
                        Ok(_) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(PostgresPersistenceError::from(RepositoryError::Conflict(
                                "partner artifact digest is already admitted".into(),
                            )))
                        }
                        Err(error) => return Err(error),
                    }
                    store_idempotency(transaction, &write.idempotency, &write.admission).await?;
                    Ok(IdempotentWrite {
                        value: write.admission,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_by_id(
        &self,
        organization_id: OrganizationId,
        id: PartnerArtifactAdmissionId,
    ) -> Result<Option<PartnerArtifactAdmission>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<AdmissionRow>(SELECT_ADMISSIONS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_row)
            .transpose()
    }

    async fn find_by_digest(
        &self,
        organization_id: OrganizationId,
        content_digest: &Sha256Digest,
    ) -> Result<Option<PartnerArtifactAdmission>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<AdmissionRow>(SELECT_ADMISSIONS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and content_digest = ")
                    .bind(content_digest.as_str()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_row)
            .transpose()
    }

    async fn list_by_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PartnerArtifactAdmission>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<AdmissionRow>(SELECT_ADMISSIONS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" order by created_at desc, id desc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(map_row)
            .collect()
    }
}
