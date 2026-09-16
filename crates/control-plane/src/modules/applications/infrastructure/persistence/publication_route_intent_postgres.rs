use super::publication_route_intent_postgres_schema::ApplicationPublicationRouteIntents;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row, transaction_error,
};
use crate::modules::applications::domain::{
    ApplicationPublicationChannel, ApplicationPublicationRateShapingPolicyRef,
    ApplicationPublicationRouteIntent, IApplicationPublicationRouteIntentRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    Database, DecodeError, Expression, FromRow, FromValue, OrderDirection, PostgresDialect,
    PostgresExecutor, PostgresTransaction, Row, SqlArray, insert_into, select_from,
};
use async_trait::async_trait;
use std::collections::BTreeSet;
use uuid::Uuid;

/// Durable Applications owner for immutable publication route intents.
#[derive(Clone)]
pub struct PostgresApplicationPublicationRouteIntentRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationPublicationRouteIntentRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationPublicationRouteIntentRepository
    for PostgresApplicationPublicationRouteIntentRepository
{
    async fn create_intent(
        &self,
        intent: ApplicationPublicationRouteIntent,
    ) -> Result<IdempotentWrite<ApplicationPublicationRouteIntent>, RepositoryError> {
        intent
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(existing) = fetch_optional::<IntentRow, _>(
                        transaction,
                        intent_query(
                            intent.organization_id,
                            intent.application_id,
                            intent.id,
                        )
                        .for_update(),
                    )
                    .await?
                    {
                        let existing = existing.intent()?;
                        if existing.project_id != intent.project_id {
                            return Err(RepositoryError::Conflict(
                                "Application publication route intent identity is already bound to another project"
                                    .into(),
                            )
                            .into());
                        }
                        if existing != intent {
                            return Err(RepositoryError::Conflict(
                                "Application publication route intent replay changed values".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    insert_intent(transaction, &intent).await?;
                    Ok(IdempotentWrite {
                        value: intent,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_intent(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        intent_id: ApplicationPublicationRouteIntentId,
    ) -> Result<Option<ApplicationPublicationRouteIntent>, RepositoryError> {
        let intent = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(intent_query(organization_id, application_id, intent_id))
            .await
            .map_err(storage)?
            .map(IntentRow::intent)
            .transpose()?;
        Ok(intent.filter(|value| value.project_id == project_id))
    }

    async fn list_intents_by_release(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        application_release_id: ApplicationReleaseId,
        application_release_digest: &Sha256Digest,
    ) -> Result<Vec<ApplicationPublicationRouteIntent>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationPublicationRouteIntents>()
                    .select(IntentSelection)
                    .filter(
                        ApplicationPublicationRouteIntents::organization_id()
                            .eq(organization_id.as_uuid()),
                    )
                    .filter(
                        ApplicationPublicationRouteIntents::project_id().eq(project_id.as_uuid()),
                    )
                    .filter(
                        ApplicationPublicationRouteIntents::application_id()
                            .eq(application_id.as_uuid()),
                    )
                    .filter(
                        ApplicationPublicationRouteIntents::application_release_id()
                            .eq(application_release_id.as_uuid()),
                    )
                    .filter(
                        ApplicationPublicationRouteIntents::application_release_digest()
                            .eq(application_release_digest.as_str()),
                    )
                    .order_by(
                        ApplicationPublicationRouteIntents::id(),
                        OrderDirection::Asc,
                    ),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(IntentRow::intent)
            .collect()
    }

    async fn list_intents_by_project(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<ApplicationPublicationRouteIntent>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationPublicationRouteIntents>()
                    .select(IntentSelection)
                    .filter(
                        ApplicationPublicationRouteIntents::organization_id()
                            .eq(organization_id.as_uuid()),
                    )
                    .filter(
                        ApplicationPublicationRouteIntents::project_id().eq(project_id.as_uuid()),
                    )
                    .order_by(
                        ApplicationPublicationRouteIntents::id(),
                        OrderDirection::Asc,
                    ),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(IntentRow::intent)
            .collect()
    }
}

fn intent_query(
    organization_id: OrganizationId,
    application_id: ApplicationId,
    intent_id: ApplicationPublicationRouteIntentId,
) -> a3s_orm::query::SelectQuery<ApplicationPublicationRouteIntents, IntentRow> {
    select_from::<ApplicationPublicationRouteIntents>()
        .select(IntentSelection)
        .filter(ApplicationPublicationRouteIntents::organization_id().eq(organization_id.as_uuid()))
        .filter(ApplicationPublicationRouteIntents::application_id().eq(application_id.as_uuid()))
        .filter(ApplicationPublicationRouteIntents::id().eq(intent_id.as_uuid()))
}

struct IntentRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    id: Uuid,
    channels: SqlArray<String>,
    embed_origin_allowlist: SqlArray<String>,
    rate_shaping_profile_id: String,
    rate_shaping_policy_revision_digest: String,
}

struct IntentSelection;

impl Selection for IntentSelection {
    type Output = IntentRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ApplicationPublicationRouteIntents::organization_id().expression(),
            ApplicationPublicationRouteIntents::project_id().expression(),
            ApplicationPublicationRouteIntents::application_id().expression(),
            ApplicationPublicationRouteIntents::application_release_id().expression(),
            ApplicationPublicationRouteIntents::application_release_digest().expression(),
            ApplicationPublicationRouteIntents::id().expression(),
            ApplicationPublicationRouteIntents::channels().expression(),
            ApplicationPublicationRouteIntents::embed_origin_allowlist().expression(),
            ApplicationPublicationRouteIntents::rate_shaping_profile_id().expression(),
            ApplicationPublicationRouteIntents::rate_shaping_policy_revision_digest().expression(),
        ]
    }
}

impl FromRow for IntentRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            application_release_id: decode(row, 3)?,
            application_release_digest: decode(row, 4)?,
            id: decode(row, 5)?,
            channels: decode(row, 6)?,
            embed_origin_allowlist: decode(row, 7)?,
            rate_shaping_profile_id: decode(row, 8)?,
            rate_shaping_policy_revision_digest: decode(row, 9)?,
        })
    }
}

impl IntentRow {
    fn intent(self) -> Result<ApplicationPublicationRouteIntent, RepositoryError> {
        let channels = self
            .channels
            .0
            .into_iter()
            .map(|channel| ApplicationPublicationChannel::parse(&channel).map_err(stored))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let intent = ApplicationPublicationRouteIntent {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            application_id: ApplicationId::from_uuid(self.application_id),
            application_release_id: ApplicationReleaseId::from_uuid(self.application_release_id),
            application_release_digest: Sha256Digest::parse(self.application_release_digest)
                .map_err(stored)?,
            id: ApplicationPublicationRouteIntentId::from_uuid(self.id),
            channels,
            embed_origin_allowlist: self.embed_origin_allowlist.0,
            rate_shaping_policy: ApplicationPublicationRateShapingPolicyRef {
                profile_id: self.rate_shaping_profile_id,
                policy_revision_digest: Sha256Digest::parse(
                    self.rate_shaping_policy_revision_digest,
                )
                .map_err(stored)?,
            },
        };
        intent.validate().map_err(stored)?;
        Ok(intent)
    }
}

async fn insert_intent(
    transaction: &PostgresTransaction,
    intent: &ApplicationPublicationRouteIntent,
) -> Result<(), PostgresPersistenceError> {
    let channels = SqlArray::from(
        intent
            .channels
            .iter()
            .map(|channel| channel.as_str().to_owned())
            .collect::<Vec<_>>(),
    );
    let embed_origin_allowlist = SqlArray::from(intent.embed_origin_allowlist.clone());
    let result = execute(
        transaction,
        insert_into::<ApplicationPublicationRouteIntents>()
            .value(
                ApplicationPublicationRouteIntents::organization_id(),
                intent.organization_id.as_uuid(),
            )
            .value(
                ApplicationPublicationRouteIntents::project_id(),
                intent.project_id.as_uuid(),
            )
            .value(
                ApplicationPublicationRouteIntents::application_id(),
                intent.application_id.as_uuid(),
            )
            .value(
                ApplicationPublicationRouteIntents::application_release_id(),
                intent.application_release_id.as_uuid(),
            )
            .value(
                ApplicationPublicationRouteIntents::application_release_digest(),
                intent.application_release_digest.as_str(),
            )
            .value(
                ApplicationPublicationRouteIntents::id(),
                intent.id.as_uuid(),
            )
            .value(ApplicationPublicationRouteIntents::channels(), channels)
            .value(
                ApplicationPublicationRouteIntents::embed_origin_allowlist(),
                embed_origin_allowlist,
            )
            .value(
                ApplicationPublicationRouteIntents::rate_shaping_profile_id(),
                intent.rate_shaping_policy.profile_id.as_str(),
            )
            .value(
                ApplicationPublicationRouteIntents::rate_shaping_policy_revision_digest(),
                intent
                    .rate_shaping_policy
                    .policy_revision_digest
                    .as_str(),
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application publication route intent", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application publication route intent identity is already in use".into(),
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
        "stored Application publication route intent is invalid: {}",
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
    fn release_list_compiles_as_one_typed_scoped_query() {
        let compiled = select_from::<ApplicationPublicationRouteIntents>()
            .select(IntentSelection)
            .filter(
                ApplicationPublicationRouteIntents::organization_id()
                    .eq(OrganizationId::new().as_uuid()),
            )
            .filter(
                ApplicationPublicationRouteIntents::project_id().eq(ProjectId::new().as_uuid()),
            )
            .filter(
                ApplicationPublicationRouteIntents::application_id()
                    .eq(ApplicationId::new().as_uuid()),
            )
            .filter(
                ApplicationPublicationRouteIntents::application_release_id()
                    .eq(ApplicationReleaseId::new().as_uuid()),
            )
            .filter(
                ApplicationPublicationRouteIntents::application_release_digest()
                    .eq("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            )
            .order_by(
                ApplicationPublicationRouteIntents::id(),
                OrderDirection::Asc,
            )
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 5);
        assert!(
            compiled
                .sql
                .contains("\"application_publication_route_intents\"")
        );
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}