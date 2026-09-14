use super::message_file_reference_postgres_schema::ApplicationMessageFileReferences;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row, transaction_error,
};
use crate::modules::applications::domain::{
    ApplicationMessageFileReference, ApplicationMessageKind,
    IApplicationMessageFileReferenceRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageFileReferenceId,
    ApplicationMessageId, ApplicationReleaseId, ApplicationSessionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError, Sha256Digest, UserFileId,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    Database, DecodeError, Expression, FromRow, FromValue, OrderDirection, PostgresDialect,
    PostgresExecutor, PostgresTransaction, Row, insert_into, select_from,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Durable Applications owner for immutable message file references.
#[derive(Clone)]
pub struct PostgresApplicationMessageFileReferenceRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationMessageFileReferenceRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationMessageFileReferenceRepository
    for PostgresApplicationMessageFileReferenceRepository
{
    async fn create_message_file_reference(
        &self,
        reference: ApplicationMessageFileReference,
    ) -> Result<IdempotentWrite<ApplicationMessageFileReference>, RepositoryError> {
        reference
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(existing) = fetch_optional::<ReferenceRow, _>(
                        transaction,
                        reference_query(
                            reference.organization_id,
                            reference.application_id,
                            reference.id,
                        )
                        .for_update(),
                    )
                    .await?
                    {
                        let existing = existing.reference()?;
                        if existing.project_id != reference.project_id {
                            return Err(RepositoryError::Conflict(
                                "Application message file reference identity is already bound to another project"
                                    .into(),
                            )
                            .into());
                        }
                        if existing != reference {
                            return Err(RepositoryError::Conflict(
                                "Application message file reference replay changed values".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    insert_reference(transaction, &reference).await?;
                    Ok(IdempotentWrite {
                        value: reference,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_message_file_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        reference_id: ApplicationMessageFileReferenceId,
    ) -> Result<Option<ApplicationMessageFileReference>, RepositoryError> {
        let reference = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(reference_query(
                organization_id,
                application_id,
                reference_id,
            ))
            .await
            .map_err(storage)?
            .map(ReferenceRow::reference)
            .transpose()?;
        Ok(reference.filter(|value| value.project_id == project_id))
    }

    async fn list_message_file_references_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageFileReference>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationMessageFileReferences>()
                    .select(ReferenceSelection)
                    .filter(
                        ApplicationMessageFileReferences::organization_id()
                            .eq(organization_id.as_uuid()),
                    )
                    .filter(
                        ApplicationMessageFileReferences::project_id().eq(project_id.as_uuid()),
                    )
                    .filter(
                        ApplicationMessageFileReferences::application_id()
                            .eq(application_id.as_uuid()),
                    )
                    .filter(
                        ApplicationMessageFileReferences::session_id().eq(session_id.as_uuid()),
                    )
                    .order_by(
                        ApplicationMessageFileReferences::created_at(),
                        OrderDirection::Asc,
                    )
                    .order_by(ApplicationMessageFileReferences::id(), OrderDirection::Asc),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(ReferenceRow::reference)
            .collect()
    }
}

fn reference_query(
    organization_id: OrganizationId,
    application_id: ApplicationId,
    reference_id: ApplicationMessageFileReferenceId,
) -> a3s_orm::query::SelectQuery<ApplicationMessageFileReferences, ReferenceRow> {
    select_from::<ApplicationMessageFileReferences>()
        .select(ReferenceSelection)
        .filter(ApplicationMessageFileReferences::organization_id().eq(organization_id.as_uuid()))
        .filter(ApplicationMessageFileReferences::application_id().eq(application_id.as_uuid()))
        .filter(ApplicationMessageFileReferences::id().eq(reference_id.as_uuid()))
}

struct ReferenceRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    session_id: Uuid,
    end_user_id: Uuid,
    invocation_id: Uuid,
    message_id: Uuid,
    message_kind: String,
    user_file_id: Uuid,
    content_digest: String,
    id: Uuid,
    created_at: DateTime<Utc>,
}

struct ReferenceSelection;

impl Selection for ReferenceSelection {
    type Output = ReferenceRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ApplicationMessageFileReferences::organization_id().expression(),
            ApplicationMessageFileReferences::project_id().expression(),
            ApplicationMessageFileReferences::application_id().expression(),
            ApplicationMessageFileReferences::application_release_id().expression(),
            ApplicationMessageFileReferences::application_release_digest().expression(),
            ApplicationMessageFileReferences::session_id().expression(),
            ApplicationMessageFileReferences::end_user_id().expression(),
            ApplicationMessageFileReferences::invocation_id().expression(),
            ApplicationMessageFileReferences::message_id().expression(),
            ApplicationMessageFileReferences::message_kind().expression(),
            ApplicationMessageFileReferences::user_file_id().expression(),
            ApplicationMessageFileReferences::content_digest().expression(),
            ApplicationMessageFileReferences::id().expression(),
            ApplicationMessageFileReferences::created_at().expression(),
        ]
    }
}

impl FromRow for ReferenceRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            application_release_id: decode(row, 3)?,
            application_release_digest: decode(row, 4)?,
            session_id: decode(row, 5)?,
            end_user_id: decode(row, 6)?,
            invocation_id: decode(row, 7)?,
            message_id: decode(row, 8)?,
            message_kind: decode(row, 9)?,
            user_file_id: decode(row, 10)?,
            content_digest: decode(row, 11)?,
            id: decode(row, 12)?,
            created_at: decode(row, 13)?,
        })
    }
}

impl ReferenceRow {
    fn reference(self) -> Result<ApplicationMessageFileReference, RepositoryError> {
        let reference = ApplicationMessageFileReference {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            application_id: ApplicationId::from_uuid(self.application_id),
            application_release_id: ApplicationReleaseId::from_uuid(self.application_release_id),
            application_release_digest: Sha256Digest::parse(self.application_release_digest)
                .map_err(stored)?,
            session_id: ApplicationSessionId::from_uuid(self.session_id),
            end_user_id: ApplicationEndUserId::from_uuid(self.end_user_id),
            invocation_id: ApplicationInvocationId::from_uuid(self.invocation_id),
            message_id: ApplicationMessageId::from_uuid(self.message_id),
            message_kind: ApplicationMessageKind::parse(&self.message_kind).map_err(stored)?,
            user_file_id: UserFileId::from_uuid(self.user_file_id),
            content_digest: Sha256Digest::parse(self.content_digest).map_err(stored)?,
            id: ApplicationMessageFileReferenceId::from_uuid(self.id),
            created_at: self.created_at,
        };
        reference.validate().map_err(stored)?;
        Ok(reference)
    }
}

async fn insert_reference(
    transaction: &PostgresTransaction,
    reference: &ApplicationMessageFileReference,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<ApplicationMessageFileReferences>()
            .value(
                ApplicationMessageFileReferences::organization_id(),
                reference.organization_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::project_id(),
                reference.project_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::application_id(),
                reference.application_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::application_release_id(),
                reference.application_release_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::application_release_digest(),
                reference.application_release_digest.as_str(),
            )
            .value(
                ApplicationMessageFileReferences::session_id(),
                reference.session_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::end_user_id(),
                reference.end_user_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::invocation_id(),
                reference.invocation_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::message_id(),
                reference.message_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::message_kind(),
                reference.message_kind.as_str(),
            )
            .value(
                ApplicationMessageFileReferences::user_file_id(),
                reference.user_file_id.as_uuid(),
            )
            .value(
                ApplicationMessageFileReferences::content_digest(),
                reference.content_digest.as_str(),
            )
            .value(ApplicationMessageFileReferences::id(), reference.id.as_uuid())
            .value(
                ApplicationMessageFileReferences::created_at(),
                reference.created_at,
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application message file reference", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application message file reference identity is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(error: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(format!(
        "stored Application message file reference is invalid: {}",
        error.into()
    ))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::Query;

    #[test]
    fn session_list_compiles_as_one_typed_scoped_query() {
        let compiled = select_from::<ApplicationMessageFileReferences>()
            .select(ReferenceSelection)
            .filter(
                ApplicationMessageFileReferences::organization_id()
                    .eq(OrganizationId::new().as_uuid()),
            )
            .filter(ApplicationMessageFileReferences::project_id().eq(ProjectId::new().as_uuid()))
            .filter(
                ApplicationMessageFileReferences::application_id()
                    .eq(ApplicationId::new().as_uuid()),
            )
            .filter(
                ApplicationMessageFileReferences::session_id()
                    .eq(ApplicationSessionId::new().as_uuid()),
            )
            .order_by(
                ApplicationMessageFileReferences::created_at(),
                OrderDirection::Asc,
            )
            .order_by(ApplicationMessageFileReferences::id(), OrderDirection::Asc)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 4);
        assert!(compiled.sql.contains("\"application_message_file_references\""));
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}
