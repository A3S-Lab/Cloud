use super::message_citation_postgres_schema::ApplicationMessageCitations;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row, transaction_error,
};
use crate::modules::applications::domain::{
    ApplicationMessageCitation, ApplicationMessageKind, IApplicationMessageCitationRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageCitationId,
    ApplicationMessageId, ApplicationReleaseId, ApplicationSessionId, IdempotentWrite,
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeChunkId, KnowledgeDocumentId,
    OrganizationId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    Database, DecodeError, Expression, FromRow, FromValue, OrderDirection, PostgresDialect,
    PostgresExecutor, PostgresTransaction, Row, insert_into, select_from,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Durable Applications owner for immutable message citations.
#[derive(Clone)]
pub struct PostgresApplicationMessageCitationRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationMessageCitationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationMessageCitationRepository for PostgresApplicationMessageCitationRepository {
    async fn create_message_citation(
        &self,
        citation: ApplicationMessageCitation,
    ) -> Result<IdempotentWrite<ApplicationMessageCitation>, RepositoryError> {
        citation
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(existing) = fetch_optional::<CitationRow, _>(
                        transaction,
                        citation_query(
                            citation.organization_id,
                            citation.application_id,
                            citation.id,
                        )
                        .for_update(),
                    )
                    .await?
                    {
                        let existing = existing.citation()?;
                        if existing.project_id != citation.project_id {
                            return Err(RepositoryError::Conflict(
                                "Application message citation identity is already bound to another project"
                                    .into(),
                            )
                            .into());
                        }
                        if existing != citation {
                            return Err(RepositoryError::Conflict(
                                "Application message citation replay changed values".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    insert_citation(transaction, &citation).await?;
                    Ok(IdempotentWrite {
                        value: citation,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_message_citation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        citation_id: ApplicationMessageCitationId,
    ) -> Result<Option<ApplicationMessageCitation>, RepositoryError> {
        let citation = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(citation_query(
                organization_id,
                application_id,
                citation_id,
            ))
            .await
            .map_err(storage)?
            .map(CitationRow::citation)
            .transpose()?;
        Ok(citation.filter(|value| value.project_id == project_id))
    }

    async fn list_message_citations_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageCitation>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationMessageCitations>()
                    .select(CitationSelection)
                    .filter(
                        ApplicationMessageCitations::organization_id()
                            .eq(organization_id.as_uuid()),
                    )
                    .filter(ApplicationMessageCitations::project_id().eq(project_id.as_uuid()))
                    .filter(
                        ApplicationMessageCitations::application_id()
                            .eq(application_id.as_uuid()),
                    )
                    .filter(ApplicationMessageCitations::session_id().eq(session_id.as_uuid()))
                    .order_by(
                        ApplicationMessageCitations::created_at(),
                        OrderDirection::Asc,
                    )
                    .order_by(ApplicationMessageCitations::id(), OrderDirection::Asc),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(CitationRow::citation)
            .collect()
    }
}

fn citation_query(
    organization_id: OrganizationId,
    application_id: ApplicationId,
    citation_id: ApplicationMessageCitationId,
) -> a3s_orm::query::SelectQuery<ApplicationMessageCitations, CitationRow> {
    select_from::<ApplicationMessageCitations>()
        .select(CitationSelection)
        .filter(ApplicationMessageCitations::organization_id().eq(organization_id.as_uuid()))
        .filter(ApplicationMessageCitations::application_id().eq(application_id.as_uuid()))
        .filter(ApplicationMessageCitations::id().eq(citation_id.as_uuid()))
}

struct CitationRow {
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
    knowledge_base_id: Uuid,
    knowledge_base_revision_id: Uuid,
    knowledge_document_id: Uuid,
    knowledge_chunk_id: Uuid,
    excerpt: Option<String>,
    excerpt_digest: String,
    id: Uuid,
    created_at: DateTime<Utc>,
}

struct CitationSelection;

impl Selection for CitationSelection {
    type Output = CitationRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ApplicationMessageCitations::organization_id().expression(),
            ApplicationMessageCitations::project_id().expression(),
            ApplicationMessageCitations::application_id().expression(),
            ApplicationMessageCitations::application_release_id().expression(),
            ApplicationMessageCitations::application_release_digest().expression(),
            ApplicationMessageCitations::session_id().expression(),
            ApplicationMessageCitations::end_user_id().expression(),
            ApplicationMessageCitations::invocation_id().expression(),
            ApplicationMessageCitations::message_id().expression(),
            ApplicationMessageCitations::message_kind().expression(),
            ApplicationMessageCitations::knowledge_base_id().expression(),
            ApplicationMessageCitations::knowledge_base_revision_id().expression(),
            ApplicationMessageCitations::knowledge_document_id().expression(),
            ApplicationMessageCitations::knowledge_chunk_id().expression(),
            ApplicationMessageCitations::excerpt().expression(),
            ApplicationMessageCitations::excerpt_digest().expression(),
            ApplicationMessageCitations::id().expression(),
            ApplicationMessageCitations::created_at().expression(),
        ]
    }
}

impl FromRow for CitationRow {
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
            knowledge_base_id: decode(row, 10)?,
            knowledge_base_revision_id: decode(row, 11)?,
            knowledge_document_id: decode(row, 12)?,
            knowledge_chunk_id: decode(row, 13)?,
            excerpt: decode(row, 14)?,
            excerpt_digest: decode(row, 15)?,
            id: decode(row, 16)?,
            created_at: decode(row, 17)?,
        })
    }
}

impl CitationRow {
    fn citation(self) -> Result<ApplicationMessageCitation, RepositoryError> {
        let citation = ApplicationMessageCitation {
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
            knowledge_base_id: KnowledgeBaseId::from_uuid(self.knowledge_base_id),
            knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(
                self.knowledge_base_revision_id,
            ),
            knowledge_document_id: KnowledgeDocumentId::from_uuid(self.knowledge_document_id),
            knowledge_chunk_id: KnowledgeChunkId::from_uuid(self.knowledge_chunk_id),
            excerpt: self.excerpt,
            excerpt_digest: Sha256Digest::parse(self.excerpt_digest).map_err(stored)?,
            id: ApplicationMessageCitationId::from_uuid(self.id),
            created_at: self.created_at,
        };
        citation.validate().map_err(stored)?;
        Ok(citation)
    }
}

async fn insert_citation(
    transaction: &PostgresTransaction,
    citation: &ApplicationMessageCitation,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<ApplicationMessageCitations>()
            .value(
                ApplicationMessageCitations::organization_id(),
                citation.organization_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::project_id(),
                citation.project_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::application_id(),
                citation.application_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::application_release_id(),
                citation.application_release_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::application_release_digest(),
                citation.application_release_digest.as_str(),
            )
            .value(
                ApplicationMessageCitations::session_id(),
                citation.session_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::end_user_id(),
                citation.end_user_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::invocation_id(),
                citation.invocation_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::message_id(),
                citation.message_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::message_kind(),
                citation.message_kind.as_str(),
            )
            .value(
                ApplicationMessageCitations::knowledge_base_id(),
                citation.knowledge_base_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::knowledge_base_revision_id(),
                citation.knowledge_base_revision_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::knowledge_document_id(),
                citation.knowledge_document_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::knowledge_chunk_id(),
                citation.knowledge_chunk_id.as_uuid(),
            )
            .value(
                ApplicationMessageCitations::excerpt(),
                citation.excerpt.clone(),
            )
            .value(
                ApplicationMessageCitations::excerpt_digest(),
                citation.excerpt_digest.as_str(),
            )
            .value(ApplicationMessageCitations::id(), citation.id.as_uuid())
            .value(
                ApplicationMessageCitations::created_at(),
                citation.created_at,
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application message citation", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application message citation identity is already in use".into(),
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
        "stored Application message citation is invalid: {}",
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
        let compiled = select_from::<ApplicationMessageCitations>()
            .select(CitationSelection)
            .filter(
                ApplicationMessageCitations::organization_id()
                    .eq(OrganizationId::new().as_uuid()),
            )
            .filter(ApplicationMessageCitations::project_id().eq(ProjectId::new().as_uuid()))
            .filter(
                ApplicationMessageCitations::application_id()
                    .eq(ApplicationId::new().as_uuid()),
            )
            .filter(
                ApplicationMessageCitations::session_id()
                    .eq(ApplicationSessionId::new().as_uuid()),
            )
            .order_by(
                ApplicationMessageCitations::created_at(),
                OrderDirection::Asc,
            )
            .order_by(ApplicationMessageCitations::id(), OrderDirection::Asc)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 4);
        assert!(compiled
            .sql
            .contains("\"application_message_citations\""));
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}

