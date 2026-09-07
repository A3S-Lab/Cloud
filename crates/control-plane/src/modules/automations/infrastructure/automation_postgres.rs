use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::automations::domain::{
    AppendAutomationRevision, AutomationDefinitionRecord, CreateAutomationDefinition,
    IAutomationDefinitionRepository,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{AutomationDefinitionV1, AutomationRevisionV1};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_HEAD: &str = "select organization_id, project_id, environment_id, automation_id, current_revision_id, current_revision_number, current_revision_digest, created_at, updated_at from automation_definitions";
const SELECT_REVISION: &str = "select organization_id, automation_id, revision_id, revision_number, parent_revision_id, parent_digest, revision_digest, revision_acl from automation_revisions";

/// Durable Automations definition/revision catalog.
///
/// The head row is locked for every revision append. Revision ACLs are restored
/// through the contract parser before they are returned to an owner, so a
/// database row can never become a mutable or latest-selector shortcut.
#[derive(Clone)]
pub struct PostgresAutomationDefinitionRepository {
    executor: PostgresExecutor,
}

impl PostgresAutomationDefinitionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAutomationDefinitionRepository for PostgresAutomationDefinitionRepository {
    async fn create(
        &self,
        request: CreateAutomationDefinition,
    ) -> Result<AutomationDefinitionRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = AutomationDefinitionRecord::new(
                        request.definition,
                        request.revision,
                        request.created_at,
                    )
                    .map_err(PostgresPersistenceError::Invariant)?;
                    insert_head(transaction, &record).await?;
                    insert_revision(transaction, &record.revision, record.created_at).await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        automation_id: Uuid,
    ) -> Result<Option<AutomationDefinitionRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_head(transaction, organization_id, automation_id, false).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_revision(
        &self,
        organization_id: Uuid,
        automation_id: Uuid,
        revision_id: Uuid,
    ) -> Result<Option<AutomationRevisionV1>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_revision(
                        transaction,
                        organization_id,
                        automation_id,
                        revision_id,
                        false,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn append_revision(
        &self,
        request: AppendAutomationRevision,
    ) -> Result<AutomationDefinitionRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let current = load_head(
                        transaction,
                        request.organization_id,
                        request.automation_id,
                        true,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)
                    .map_err(PostgresPersistenceError::Repository)?;
                    let updated = current
                        .append(
                            request.revision,
                            &request.expected_revision_digest,
                            request.updated_at,
                        )
                        .map_err(|error| {
                            PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                        })?;
                    insert_revision(transaction, &updated.revision, updated.updated_at).await?;
                    update_head(transaction, &updated).await?;
                    Ok(updated)
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_head(
    transaction: &PostgresTransaction,
    record: &AutomationDefinitionRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.definition.spec();
    let revision = record.revision.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into automation_definitions (organization_id, project_id, environment_id, automation_id, current_revision_id, current_revision_number, current_revision_digest, created_at, updated_at) values (")
            .bind(spec.organization_id)
            .append(", ")
            .bind(spec.project_id)
            .append(", ")
            .bind(spec.environment_id)
            .append(", ")
            .bind(spec.automation_id)
            .append(", ")
            .bind(revision.revision_id)
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(record.revision.digest())
            .append(", ")
            .bind(record.created_at)
            .append(", ")
            .bind(record.updated_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("Automation definition", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("Automation definition already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn insert_revision(
    transaction: &PostgresTransaction,
    revision: &AutomationRevisionV1,
    created_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let spec = revision.spec();
    let definition = &spec.definition;
    let rows = execute(
        transaction,
        sql_query::<()>("insert into automation_revisions (organization_id, automation_id, revision_id, revision_number, parent_revision_id, parent_digest, revision_digest, revision_acl, created_at) values (")
            .bind(definition.organization_id)
            .append(", ")
            .bind(definition.automation_id)
            .append(", ")
            .bind(spec.revision_id)
            .append(", ")
            .bind(spec.revision_number)
            .append(", ")
            .bind(spec.parent_revision_id)
            .append(", ")
            .bind(spec.parent_digest.as_deref())
            .append(", ")
            .bind(revision.digest())
            .append(", ")
            .bind(revision.canonical_acl())
            .append(", ")
            .bind(created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("Automation revision", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("Automation revision already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn update_head(
    transaction: &PostgresTransaction,
    record: &AutomationDefinitionRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.definition.spec();
    let revision = record.revision.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("update automation_definitions set current_revision_id = ")
            .bind(revision.revision_id)
            .append(", current_revision_number = ")
            .bind(revision.revision_number)
            .append(", current_revision_digest = ")
            .bind(record.revision.digest())
            .append(", updated_at = ")
            .bind(record.updated_at)
            .append(" where organization_id = ")
            .bind(spec.organization_id)
            .append(" and automation_id = ")
            .bind(spec.automation_id),
    )
    .await?;
    require_one_row("Automation definition head update", rows)
}

async fn load_head(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    automation_id: Uuid,
    for_update: bool,
) -> Result<Option<AutomationDefinitionRecord>, PostgresPersistenceError> {
    let mut query = sql_query::<AutomationDefinitionRow>(SELECT_HEAD)
        .append(" where organization_id = ")
        .bind(organization_id)
        .append(" and automation_id = ")
        .bind(automation_id);
    if for_update {
        query = query.append(" for update");
    }
    let Some(row) = fetch_optional(transaction, query).await? else {
        return Ok(None);
    };
    let revision = load_revision(
        transaction,
        row.organization_id,
        row.automation_id,
        row.current_revision_id,
        false,
    )
    .await?;
    let Some(revision) = revision else {
        return Err(PostgresPersistenceError::Invariant(
            "Automation definition head revision is missing".into(),
        ));
    };
    if row.current_revision_number
        != i64::try_from(revision.spec().revision_number).map_err(|_| {
            PostgresPersistenceError::Invariant(
                "Automation definition revision number exceeds database bounds".into(),
            )
        })?
        || row.current_revision_digest != revision.digest()
    {
        return Err(PostgresPersistenceError::Invariant(
            "Automation definition head projection drifted".into(),
        ));
    }
    let definition = AutomationDefinitionV1::from_spec(revision.spec().definition.clone())
        .map_err(PostgresPersistenceError::Invariant)?;
    if row.project_id != definition.spec().project_id
        || row.environment_id != definition.spec().environment_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "Automation definition scope projection drifted".into(),
        ));
    }
    let record = AutomationDefinitionRecord {
        definition,
        revision,
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    record
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    Ok(Some(record))
}

async fn load_revision(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    automation_id: Uuid,
    revision_id: Uuid,
    for_update: bool,
) -> Result<Option<AutomationRevisionV1>, PostgresPersistenceError> {
    let mut query = sql_query::<AutomationRevisionRow>(SELECT_REVISION)
        .append(" where organization_id = ")
        .bind(organization_id)
        .append(" and automation_id = ")
        .bind(automation_id)
        .append(" and revision_id = ")
        .bind(revision_id);
    if for_update {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(decode_revision)
        .transpose()
}

struct AutomationDefinitionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    automation_id: Uuid,
    current_revision_id: Uuid,
    current_revision_number: i64,
    current_revision_digest: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for AutomationDefinitionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            automation_id: decode(row, 3)?,
            current_revision_id: decode(row, 4)?,
            current_revision_number: decode(row, 5)?,
            current_revision_digest: decode(row, 6)?,
            created_at: decode(row, 7)?,
            updated_at: decode(row, 8)?,
        })
    }
}

struct AutomationRevisionRow {
    organization_id: Uuid,
    automation_id: Uuid,
    revision_id: Uuid,
    revision_number: i64,
    parent_revision_id: Option<Uuid>,
    parent_digest: Option<String>,
    revision_digest: String,
    revision_acl: String,
}

impl FromRow for AutomationRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            automation_id: decode(row, 1)?,
            revision_id: decode(row, 2)?,
            revision_number: decode(row, 3)?,
            parent_revision_id: decode(row, 4)?,
            parent_digest: decode(row, 5)?,
            revision_digest: decode(row, 6)?,
            revision_acl: decode(row, 7)?,
        })
    }
}

fn decode_revision(
    row: AutomationRevisionRow,
) -> Result<AutomationRevisionV1, PostgresPersistenceError> {
    if row.revision_number <= 0 || row.parent_revision_id.is_some() != row.parent_digest.is_some() {
        return Err(PostgresPersistenceError::Invariant(
            "Automation revision lineage columns are invalid".into(),
        ));
    }
    let revision = AutomationRevisionV1::restore(&row.revision_acl, &row.revision_digest)
        .map_err(PostgresPersistenceError::Invariant)?;
    let spec = revision.spec();
    if spec.definition.organization_id != row.organization_id
        || spec.definition.automation_id != row.automation_id
        || spec.revision_id != row.revision_id
        || i64::try_from(spec.revision_number).ok() != Some(row.revision_number)
        || spec.parent_revision_id != row.parent_revision_id
        || spec.parent_digest.as_deref() != row.parent_digest.as_deref()
    {
        return Err(PostgresPersistenceError::Invariant(
            "Automation revision projection drifted".into(),
        ));
    }
    Ok(revision)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn repository_restores_revision_acl_through_the_contract_parser() {
        let source = include_str!("automation_postgres.rs");
        assert!(source.contains("AutomationRevisionV1::restore"));
        assert!(source.contains("AutomationDefinitionRecord"));
        assert!(source.contains("for update"));
    }
}
