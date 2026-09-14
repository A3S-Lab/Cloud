use super::message_variant_postgres_schema::ApplicationMessageVariants;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row, transaction_error,
};
use crate::modules::applications::domain::{
    ApplicationMessageKind, ApplicationMessageVariant, IApplicationMessageVariantRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationMessageVariantId, ApplicationReleaseId, ApplicationSessionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError, Sha256Digest,
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

/// Durable Applications owner for immutable session message variants.
#[derive(Clone)]
pub struct PostgresApplicationMessageVariantRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationMessageVariantRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationMessageVariantRepository for PostgresApplicationMessageVariantRepository {
    async fn create_message_variant(
        &self,
        variant: ApplicationMessageVariant,
    ) -> Result<IdempotentWrite<ApplicationMessageVariant>, RepositoryError> {
        variant
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(existing) = fetch_optional::<VariantRow, _>(
                        transaction,
                        variant_query(
                            variant.organization_id,
                            variant.application_id,
                            variant.id,
                        )
                        .for_update(),
                    )
                    .await?
                    {
                        let existing = existing.variant()?;
                        if existing.project_id != variant.project_id {
                            return Err(RepositoryError::Conflict(
                                "Application message variant identity is already bound to another project"
                                    .into(),
                            )
                            .into());
                        }
                        if existing != variant {
                            return Err(RepositoryError::Conflict(
                                "Application message variant replay changed values".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    insert_variant(transaction, &variant).await?;
                    Ok(IdempotentWrite {
                        value: variant,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_message_variant(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        variant_id: ApplicationMessageVariantId,
    ) -> Result<Option<ApplicationMessageVariant>, RepositoryError> {
        let variant = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(variant_query(organization_id, application_id, variant_id))
            .await
            .map_err(storage)?
            .map(VariantRow::variant)
            .transpose()?;
        Ok(variant.filter(|value| value.project_id == project_id))
    }

    async fn list_message_variants_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageVariant>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationMessageVariants>()
                    .select(VariantSelection)
                    .filter(
                        ApplicationMessageVariants::organization_id().eq(organization_id.as_uuid()),
                    )
                    .filter(ApplicationMessageVariants::project_id().eq(project_id.as_uuid()))
                    .filter(
                        ApplicationMessageVariants::application_id().eq(application_id.as_uuid()),
                    )
                    .filter(ApplicationMessageVariants::session_id().eq(session_id.as_uuid()))
                    .order_by(
                        ApplicationMessageVariants::created_at(),
                        OrderDirection::Asc,
                    )
                    .order_by(ApplicationMessageVariants::id(), OrderDirection::Asc),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(VariantRow::variant)
            .collect()
    }
}

fn variant_query(
    organization_id: OrganizationId,
    application_id: ApplicationId,
    variant_id: ApplicationMessageVariantId,
) -> a3s_orm::query::SelectQuery<ApplicationMessageVariants, VariantRow> {
    select_from::<ApplicationMessageVariants>()
        .select(VariantSelection)
        .filter(ApplicationMessageVariants::organization_id().eq(organization_id.as_uuid()))
        .filter(ApplicationMessageVariants::application_id().eq(application_id.as_uuid()))
        .filter(ApplicationMessageVariants::id().eq(variant_id.as_uuid()))
}

struct VariantRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    session_id: Uuid,
    end_user_id: Uuid,
    invocation_id: Uuid,
    source_message_id: Uuid,
    source_message_kind: String,
    id: Uuid,
    instruction: Option<Value>,
    instruction_digest: String,
    created_at: DateTime<Utc>,
}

struct VariantSelection;

impl Selection for VariantSelection {
    type Output = VariantRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ApplicationMessageVariants::organization_id().expression(),
            ApplicationMessageVariants::project_id().expression(),
            ApplicationMessageVariants::application_id().expression(),
            ApplicationMessageVariants::application_release_id().expression(),
            ApplicationMessageVariants::application_release_digest().expression(),
            ApplicationMessageVariants::session_id().expression(),
            ApplicationMessageVariants::end_user_id().expression(),
            ApplicationMessageVariants::invocation_id().expression(),
            ApplicationMessageVariants::source_message_id().expression(),
            ApplicationMessageVariants::source_message_kind().expression(),
            ApplicationMessageVariants::id().expression(),
            ApplicationMessageVariants::instruction().expression(),
            ApplicationMessageVariants::instruction_digest().expression(),
            ApplicationMessageVariants::created_at().expression(),
        ]
    }
}

impl FromRow for VariantRow {
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
            source_message_id: decode(row, 8)?,
            source_message_kind: decode(row, 9)?,
            id: decode(row, 10)?,
            instruction: decode(row, 11)?,
            instruction_digest: decode(row, 12)?,
            created_at: decode(row, 13)?,
        })
    }
}

impl VariantRow {
    fn variant(self) -> Result<ApplicationMessageVariant, RepositoryError> {
        let variant = ApplicationMessageVariant {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            application_id: ApplicationId::from_uuid(self.application_id),
            application_release_id: ApplicationReleaseId::from_uuid(self.application_release_id),
            application_release_digest: Sha256Digest::parse(self.application_release_digest)
                .map_err(stored)?,
            session_id: ApplicationSessionId::from_uuid(self.session_id),
            end_user_id: ApplicationEndUserId::from_uuid(self.end_user_id),
            invocation_id: ApplicationInvocationId::from_uuid(self.invocation_id),
            source_message_id: ApplicationMessageId::from_uuid(self.source_message_id),
            source_message_kind: ApplicationMessageKind::parse(&self.source_message_kind)
                .map_err(stored)?,
            id: ApplicationMessageVariantId::from_uuid(self.id),
            instruction: self.instruction,
            instruction_digest: Sha256Digest::parse(self.instruction_digest).map_err(stored)?,
            created_at: self.created_at,
        };
        variant.validate().map_err(stored)?;
        Ok(variant)
    }
}

async fn insert_variant(
    transaction: &PostgresTransaction,
    variant: &ApplicationMessageVariant,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<ApplicationMessageVariants>()
            .value(
                ApplicationMessageVariants::organization_id(),
                variant.organization_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::project_id(),
                variant.project_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::application_id(),
                variant.application_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::application_release_id(),
                variant.application_release_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::application_release_digest(),
                variant.application_release_digest.as_str(),
            )
            .value(
                ApplicationMessageVariants::session_id(),
                variant.session_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::end_user_id(),
                variant.end_user_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::invocation_id(),
                variant.invocation_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::source_message_id(),
                variant.source_message_id.as_uuid(),
            )
            .value(
                ApplicationMessageVariants::source_message_kind(),
                variant.source_message_kind.as_str(),
            )
            .value(ApplicationMessageVariants::id(), variant.id.as_uuid())
            .value(
                ApplicationMessageVariants::instruction(),
                variant.instruction.clone(),
            )
            .value(
                ApplicationMessageVariants::instruction_digest(),
                variant.instruction_digest.as_str(),
            )
            .value(ApplicationMessageVariants::created_at(), variant.created_at),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application message variant", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application message variant identity is already in use".into(),
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
        "stored Application message variant is invalid: {}",
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
        let compiled = select_from::<ApplicationMessageVariants>()
            .select(VariantSelection)
            .filter(
                ApplicationMessageVariants::organization_id().eq(OrganizationId::new().as_uuid()),
            )
            .filter(ApplicationMessageVariants::project_id().eq(ProjectId::new().as_uuid()))
            .filter(ApplicationMessageVariants::application_id().eq(ApplicationId::new().as_uuid()))
            .filter(
                ApplicationMessageVariants::session_id().eq(ApplicationSessionId::new().as_uuid()),
            )
            .order_by(
                ApplicationMessageVariants::created_at(),
                OrderDirection::Asc,
            )
            .order_by(ApplicationMessageVariants::id(), OrderDirection::Asc)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 4);
        assert!(compiled.sql.contains("\"application_message_variants\""));
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}
