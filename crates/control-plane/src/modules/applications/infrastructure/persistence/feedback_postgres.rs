use super::feedback_postgres_schema::ApplicationFeedbacks;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row, transaction_error,
};
use crate::modules::applications::domain::{
    ApplicationFeedback, ApplicationFeedbackRating, IApplicationFeedbackRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationFeedbackId, ApplicationId, ApplicationMessageId,
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
use uuid::Uuid;

/// Durable Applications owner for immutable session feedback.
#[derive(Clone)]
pub struct PostgresApplicationFeedbackRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationFeedbackRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationFeedbackRepository for PostgresApplicationFeedbackRepository {
    async fn create_feedback(
        &self,
        feedback: ApplicationFeedback,
    ) -> Result<IdempotentWrite<ApplicationFeedback>, RepositoryError> {
        feedback
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(existing) = fetch_optional::<FeedbackRow, _>(
                        transaction,
                        feedback_query(
                            feedback.organization_id,
                            feedback.application_id,
                            feedback.id,
                        )
                        .for_update(),
                    )
                    .await?
                    {
                        let existing = existing.feedback()?;
                        if existing.project_id != feedback.project_id {
                            return Err(RepositoryError::Conflict(
                                "Application feedback identity is already bound to another project"
                                    .into(),
                            )
                            .into());
                        }
                        if existing != feedback {
                            return Err(RepositoryError::Conflict(
                                "Application feedback replay changed values".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    insert_feedback(transaction, &feedback).await?;
                    Ok(IdempotentWrite {
                        value: feedback,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_feedback(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        feedback_id: ApplicationFeedbackId,
    ) -> Result<Option<ApplicationFeedback>, RepositoryError> {
        let feedback = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(feedback_query(organization_id, application_id, feedback_id))
            .await
            .map_err(storage)?
            .map(FeedbackRow::feedback)
            .transpose()?;
        Ok(feedback.filter(|value| value.project_id == project_id))
    }

    async fn list_feedback_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationFeedback>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationFeedbacks>()
                    .select(FeedbackSelection)
                    .filter(ApplicationFeedbacks::organization_id().eq(organization_id.as_uuid()))
                    .filter(ApplicationFeedbacks::project_id().eq(project_id.as_uuid()))
                    .filter(ApplicationFeedbacks::application_id().eq(application_id.as_uuid()))
                    .filter(ApplicationFeedbacks::session_id().eq(session_id.as_uuid()))
                    .order_by(ApplicationFeedbacks::created_at(), OrderDirection::Asc)
                    .order_by(ApplicationFeedbacks::id(), OrderDirection::Asc),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(FeedbackRow::feedback)
            .collect()
    }
}

fn feedback_query(
    organization_id: OrganizationId,
    application_id: ApplicationId,
    feedback_id: ApplicationFeedbackId,
) -> a3s_orm::query::SelectQuery<ApplicationFeedbacks, FeedbackRow> {
    select_from::<ApplicationFeedbacks>()
        .select(FeedbackSelection)
        .filter(ApplicationFeedbacks::organization_id().eq(organization_id.as_uuid()))
        .filter(ApplicationFeedbacks::application_id().eq(application_id.as_uuid()))
        .filter(ApplicationFeedbacks::id().eq(feedback_id.as_uuid()))
}

struct FeedbackRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    session_id: Uuid,
    end_user_id: Uuid,
    source_message_id: Option<Uuid>,
    id: Uuid,
    rating: String,
    comment: Option<String>,
    content_digest: String,
    created_at: DateTime<Utc>,
}

struct FeedbackSelection;

impl Selection for FeedbackSelection {
    type Output = FeedbackRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ApplicationFeedbacks::organization_id().expression(),
            ApplicationFeedbacks::project_id().expression(),
            ApplicationFeedbacks::application_id().expression(),
            ApplicationFeedbacks::application_release_id().expression(),
            ApplicationFeedbacks::application_release_digest().expression(),
            ApplicationFeedbacks::session_id().expression(),
            ApplicationFeedbacks::end_user_id().expression(),
            ApplicationFeedbacks::source_message_id().expression(),
            ApplicationFeedbacks::id().expression(),
            ApplicationFeedbacks::rating().expression(),
            ApplicationFeedbacks::comment().expression(),
            ApplicationFeedbacks::content_digest().expression(),
            ApplicationFeedbacks::created_at().expression(),
        ]
    }
}

impl FromRow for FeedbackRow {
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
            rating: decode(row, 9)?,
            comment: decode(row, 10)?,
            content_digest: decode(row, 11)?,
            created_at: decode(row, 12)?,
        })
    }
}

impl FeedbackRow {
    fn feedback(self) -> Result<ApplicationFeedback, RepositoryError> {
        let feedback = ApplicationFeedback {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            application_id: ApplicationId::from_uuid(self.application_id),
            application_release_id: ApplicationReleaseId::from_uuid(self.application_release_id),
            application_release_digest: Sha256Digest::parse(self.application_release_digest)
                .map_err(stored)?,
            session_id: ApplicationSessionId::from_uuid(self.session_id),
            end_user_id: ApplicationEndUserId::from_uuid(self.end_user_id),
            source_message_id: self.source_message_id.map(ApplicationMessageId::from_uuid),
            id: ApplicationFeedbackId::from_uuid(self.id),
            rating: ApplicationFeedbackRating::parse(&self.rating).map_err(stored)?,
            comment: self.comment,
            content_digest: Sha256Digest::parse(self.content_digest).map_err(stored)?,
            created_at: self.created_at,
        };
        feedback.validate().map_err(stored)?;
        Ok(feedback)
    }
}

async fn insert_feedback(
    transaction: &PostgresTransaction,
    feedback: &ApplicationFeedback,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<ApplicationFeedbacks>()
            .value(
                ApplicationFeedbacks::organization_id(),
                feedback.organization_id.as_uuid(),
            )
            .value(
                ApplicationFeedbacks::project_id(),
                feedback.project_id.as_uuid(),
            )
            .value(
                ApplicationFeedbacks::application_id(),
                feedback.application_id.as_uuid(),
            )
            .value(
                ApplicationFeedbacks::application_release_id(),
                feedback.application_release_id.as_uuid(),
            )
            .value(
                ApplicationFeedbacks::application_release_digest(),
                feedback.application_release_digest.as_str(),
            )
            .value(
                ApplicationFeedbacks::session_id(),
                feedback.session_id.as_uuid(),
            )
            .value(
                ApplicationFeedbacks::end_user_id(),
                feedback.end_user_id.as_uuid(),
            )
            .value(
                ApplicationFeedbacks::source_message_id(),
                feedback.source_message_id.map(|id| id.as_uuid()),
            )
            .value(ApplicationFeedbacks::id(), feedback.id.as_uuid())
            .value(ApplicationFeedbacks::rating(), feedback.rating.as_str())
            .value(ApplicationFeedbacks::comment(), feedback.comment.clone())
            .value(
                ApplicationFeedbacks::content_digest(),
                feedback.content_digest.as_str(),
            )
            .value(ApplicationFeedbacks::created_at(), feedback.created_at),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application feedback", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application feedback identity is already in use".into(),
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
        "stored Application feedback is invalid: {}",
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
        let compiled = select_from::<ApplicationFeedbacks>()
            .select(FeedbackSelection)
            .filter(ApplicationFeedbacks::organization_id().eq(OrganizationId::new().as_uuid()))
            .filter(ApplicationFeedbacks::project_id().eq(ProjectId::new().as_uuid()))
            .filter(ApplicationFeedbacks::application_id().eq(ApplicationId::new().as_uuid()))
            .filter(ApplicationFeedbacks::session_id().eq(ApplicationSessionId::new().as_uuid()))
            .order_by(ApplicationFeedbacks::created_at(), OrderDirection::Asc)
            .order_by(ApplicationFeedbacks::id(), OrderDirection::Asc)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 4);
        assert!(compiled.sql.contains("\"application_feedbacks\""));
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}
