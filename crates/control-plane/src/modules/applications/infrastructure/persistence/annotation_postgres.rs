use super::annotation_postgres_schema::ApplicationAnnotations;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row, transaction_error,
};
use crate::modules::applications::domain::{
    ApplicationAnnotation, IApplicationAnnotationRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationAnnotationId, ApplicationEndUserId, ApplicationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, IdempotentWrite, OrganizationId, ProjectId,
    RepositoryError, Sha256Digest,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    Database, DecodeError, Expression, FromRow, FromValue, OrderDirection, PostgresDialect,
    PostgresExecutor, PostgresTransaction, Row, insert_into, select_from,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

/// Durable Applications owner for immutable session annotation.
#[derive(Clone)]
pub struct PostgresApplicationAnnotationRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationAnnotationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationAnnotationRepository for PostgresApplicationAnnotationRepository {
    async fn create_annotation(
        &self,
        annotation: ApplicationAnnotation,
    ) -> Result<IdempotentWrite<ApplicationAnnotation>, RepositoryError> {
        annotation
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(existing) = fetch_optional::<AnnotationRow, _>(
                        transaction,
                        annotation_query(
                            annotation.organization_id,
                            annotation.application_id,
                            annotation.id,
                        )
                        .for_update(),
                    )
                    .await?
                    {
                        let existing = existing.annotation()?;
                        if existing.project_id != annotation.project_id {
                            return Err(RepositoryError::Conflict(
                                "Application annotation identity is already bound to another project"
                                    .into(),
                            )
                            .into());
                        }
                        if existing != annotation {
                            return Err(RepositoryError::Conflict(
                                "Application annotation replay changed values".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    insert_annotation(transaction, &annotation).await?;
                    Ok(IdempotentWrite {
                        value: annotation,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_annotation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        annotation_id: ApplicationAnnotationId,
    ) -> Result<Option<ApplicationAnnotation>, RepositoryError> {
        let annotation = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(annotation_query(
                organization_id,
                application_id,
                annotation_id,
            ))
            .await
            .map_err(storage)?
            .map(AnnotationRow::annotation)
            .transpose()?;
        Ok(annotation.filter(|value| value.project_id == project_id))
    }

    async fn list_annotations_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationAnnotation>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationAnnotations>()
                    .select(AnnotationSelection)
                    .filter(ApplicationAnnotations::organization_id().eq(organization_id.as_uuid()))
                    .filter(ApplicationAnnotations::project_id().eq(project_id.as_uuid()))
                    .filter(ApplicationAnnotations::application_id().eq(application_id.as_uuid()))
                    .filter(ApplicationAnnotations::session_id().eq(session_id.as_uuid()))
                    .order_by(ApplicationAnnotations::created_at(), OrderDirection::Asc)
                    .order_by(ApplicationAnnotations::id(), OrderDirection::Asc),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(AnnotationRow::annotation)
            .collect()
    }
}

fn annotation_query(
    organization_id: OrganizationId,
    application_id: ApplicationId,
    annotation_id: ApplicationAnnotationId,
) -> a3s_orm::query::SelectQuery<ApplicationAnnotations, AnnotationRow> {
    select_from::<ApplicationAnnotations>()
        .select(AnnotationSelection)
        .filter(ApplicationAnnotations::organization_id().eq(organization_id.as_uuid()))
        .filter(ApplicationAnnotations::application_id().eq(application_id.as_uuid()))
        .filter(ApplicationAnnotations::id().eq(annotation_id.as_uuid()))
}

struct AnnotationRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    session_id: Uuid,
    end_user_id: Uuid,
    source_message_id: Option<Uuid>,
    id: Uuid,
    content: Value,
    content_digest: String,
    created_at: DateTime<Utc>,
}

struct AnnotationSelection;

impl Selection for AnnotationSelection {
    type Output = AnnotationRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ApplicationAnnotations::organization_id().expression(),
            ApplicationAnnotations::project_id().expression(),
            ApplicationAnnotations::application_id().expression(),
            ApplicationAnnotations::application_release_id().expression(),
            ApplicationAnnotations::application_release_digest().expression(),
            ApplicationAnnotations::session_id().expression(),
            ApplicationAnnotations::end_user_id().expression(),
            ApplicationAnnotations::source_message_id().expression(),
            ApplicationAnnotations::id().expression(),
            ApplicationAnnotations::content().expression(),
            ApplicationAnnotations::content_digest().expression(),
            ApplicationAnnotations::created_at().expression(),
        ]
    }
}

impl FromRow for AnnotationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            application_release_id: decode(row, 3)?,
            application_release_digest: decode(row, 4)?,
            session_id: decode(row, 5)?,
            end_user_id: decode(row, 6)?,
            source_message_id: decode(row, 7)?,
            id: decode(row, 8)?,
            content: decode(row, 9)?,
            content_digest: decode(row, 10)?,
            created_at: decode(row, 11)?,
        })
    }
}

impl AnnotationRow {
    fn annotation(self) -> Result<ApplicationAnnotation, RepositoryError> {
        let annotation = ApplicationAnnotation {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            application_id: ApplicationId::from_uuid(self.application_id),
            application_release_id: ApplicationReleaseId::from_uuid(self.application_release_id),
            application_release_digest: Sha256Digest::parse(self.application_release_digest)
                .map_err(stored)?,
            session_id: ApplicationSessionId::from_uuid(self.session_id),
            end_user_id: ApplicationEndUserId::from_uuid(self.end_user_id),
            source_message_id: self.source_message_id.map(ApplicationMessageId::from_uuid),
            id: ApplicationAnnotationId::from_uuid(self.id),
            content: self.content,
            content_digest: Sha256Digest::parse(self.content_digest).map_err(stored)?,
            created_at: self.created_at,
        };
        annotation.validate().map_err(stored)?;
        Ok(annotation)
    }
}

async fn insert_annotation(
    transaction: &PostgresTransaction,
    annotation: &ApplicationAnnotation,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<ApplicationAnnotations>()
            .value(
                ApplicationAnnotations::organization_id(),
                annotation.organization_id.as_uuid(),
            )
            .value(
                ApplicationAnnotations::project_id(),
                annotation.project_id.as_uuid(),
            )
            .value(
                ApplicationAnnotations::application_id(),
                annotation.application_id.as_uuid(),
            )
            .value(
                ApplicationAnnotations::application_release_id(),
                annotation.application_release_id.as_uuid(),
            )
            .value(
                ApplicationAnnotations::application_release_digest(),
                annotation.application_release_digest.as_str(),
            )
            .value(
                ApplicationAnnotations::session_id(),
                annotation.session_id.as_uuid(),
            )
            .value(
                ApplicationAnnotations::end_user_id(),
                annotation.end_user_id.as_uuid(),
            )
            .value(
                ApplicationAnnotations::source_message_id(),
                annotation.source_message_id.map(|id| id.as_uuid()),
            )
            .value(ApplicationAnnotations::id(), annotation.id.as_uuid())
            .value(
                ApplicationAnnotations::content(),
                annotation.content.clone(),
            )
            .value(
                ApplicationAnnotations::content_digest(),
                annotation.content_digest.as_str(),
            )
            .value(ApplicationAnnotations::created_at(), annotation.created_at),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application annotation", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application annotation identity is already in use".into(),
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
        "stored Application annotation is invalid: {}",
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
        let compiled = select_from::<ApplicationAnnotations>()
            .select(AnnotationSelection)
            .filter(ApplicationAnnotations::organization_id().eq(OrganizationId::new().as_uuid()))
            .filter(ApplicationAnnotations::project_id().eq(ProjectId::new().as_uuid()))
            .filter(ApplicationAnnotations::application_id().eq(ApplicationId::new().as_uuid()))
            .filter(ApplicationAnnotations::session_id().eq(ApplicationSessionId::new().as_uuid()))
            .order_by(ApplicationAnnotations::created_at(), OrderDirection::Asc)
            .order_by(ApplicationAnnotations::id(), OrderDirection::Asc)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 4);
        assert!(compiled.sql.contains("\"application_annotations\""));
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}
