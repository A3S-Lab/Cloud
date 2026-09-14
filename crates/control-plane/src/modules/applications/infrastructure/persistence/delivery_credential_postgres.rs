use super::delivery_credential_postgres_schema::ApplicationDeliveryCredentials;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row, transaction_error,
};
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryCredential, ApplicationDeliveryCredentialStatus,
    IApplicationDeliveryCredentialRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, SecretId, SecretVersionReference,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    Database, DecodeError, Expression, FromRow, FromValue, OrderDirection, PostgresDialect,
    PostgresExecutor, PostgresTransaction, Row, insert_into, select_from, update_table,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Durable Applications owner for anonymous delivery credential bindings.
#[derive(Clone)]
pub struct PostgresApplicationDeliveryCredentialRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationDeliveryCredentialRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationDeliveryCredentialRepository for PostgresApplicationDeliveryCredentialRepository {
    async fn create_delivery_credential(
        &self,
        credential: ApplicationDeliveryCredential,
    ) -> Result<ApplicationDeliveryCredential, RepositoryError> {
        credential
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        if credential.generation != 1
            || credential.created_at != credential.updated_at
            || credential.revoked_at.is_some()
        {
            return Err(RepositoryError::Conflict(
                "new Application delivery credential is not at its initial generation".into(),
            ));
        }
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    insert_credential(transaction, &credential).await?;
                    Ok(credential)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn update_delivery_credential(
        &self,
        credential: ApplicationDeliveryCredential,
        expected_generation: u64,
    ) -> Result<ApplicationDeliveryCredential, RepositoryError> {
        if expected_generation == 0
            || expected_generation.checked_add(1) != Some(credential.generation)
        {
            return Err(RepositoryError::Conflict(
                "Application delivery credential generation transition is invalid".into(),
            ));
        }
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let existing = fetch_optional::<DeliveryCredentialRow, _>(
                        transaction,
                        credential_query(
                            credential.organization_id,
                            credential.application_id,
                            credential.id,
                        )
                        .for_update(),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?
                    .credential()?;
                    if existing.project_id != credential.project_id {
                        return Err(RepositoryError::NotFound.into());
                    }
                    credential
                        .validate_transition_from(&existing, expected_generation)
                        .map_err(RepositoryError::Conflict)?;
                    update_credential_row(transaction, &credential, expected_generation).await?;
                    Ok(credential)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_delivery_credential(
        &self,
        organization_id: OrganizationId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
    ) -> Result<Option<ApplicationDeliveryCredential>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(credential_query(
                organization_id,
                application_id,
                credential_id,
            ))
            .await
            .map_err(storage)?
            .map(DeliveryCredentialRow::credential)
            .transpose()
    }

    async fn find_delivery_credential_by_lookup_key(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        lookup_key: &str,
    ) -> Result<Option<ApplicationDeliveryCredential>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                select_from::<ApplicationDeliveryCredentials>()
                    .select(DeliveryCredentialSelection)
                    .filter(
                        ApplicationDeliveryCredentials::organization_id()
                            .eq(organization_id.as_uuid()),
                    )
                    .filter(ApplicationDeliveryCredentials::project_id().eq(project_id.as_uuid()))
                    .filter(
                        ApplicationDeliveryCredentials::application_id()
                            .eq(application_id.as_uuid()),
                    )
                    .filter(ApplicationDeliveryCredentials::lookup_key().eq(lookup_key)),
            )
            .await
            .map_err(storage)?
            .map(DeliveryCredentialRow::credential)
            .transpose()
    }

    async fn list_delivery_credentials_by_application(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> Result<Vec<ApplicationDeliveryCredential>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<ApplicationDeliveryCredentials>()
                    .select(DeliveryCredentialSelection)
                    .filter(
                        ApplicationDeliveryCredentials::organization_id()
                            .eq(organization_id.as_uuid()),
                    )
                    .filter(ApplicationDeliveryCredentials::project_id().eq(project_id.as_uuid()))
                    .filter(
                        ApplicationDeliveryCredentials::application_id()
                            .eq(application_id.as_uuid()),
                    )
                    .order_by(
                        ApplicationDeliveryCredentials::created_at(),
                        OrderDirection::Asc,
                    )
                    .order_by(ApplicationDeliveryCredentials::id(), OrderDirection::Asc),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(DeliveryCredentialRow::credential)
            .collect()
    }
}

fn credential_query(
    organization_id: OrganizationId,
    application_id: ApplicationId,
    credential_id: ApplicationDeliveryCredentialId,
) -> a3s_orm::query::SelectQuery<ApplicationDeliveryCredentials, DeliveryCredentialRow> {
    select_from::<ApplicationDeliveryCredentials>()
        .select(DeliveryCredentialSelection)
        .filter(ApplicationDeliveryCredentials::organization_id().eq(organization_id.as_uuid()))
        .filter(ApplicationDeliveryCredentials::application_id().eq(application_id.as_uuid()))
        .filter(ApplicationDeliveryCredentials::id().eq(credential_id.as_uuid()))
}

struct DeliveryCredentialRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    id: Uuid,
    audience: String,
    lookup_key: String,
    secret_id: Uuid,
    secret_version: u64,
    generation: u64,
    status: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

struct DeliveryCredentialSelection;

impl Selection for DeliveryCredentialSelection {
    type Output = DeliveryCredentialRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ApplicationDeliveryCredentials::organization_id().expression(),
            ApplicationDeliveryCredentials::project_id().expression(),
            ApplicationDeliveryCredentials::application_id().expression(),
            ApplicationDeliveryCredentials::id().expression(),
            ApplicationDeliveryCredentials::audience().expression(),
            ApplicationDeliveryCredentials::lookup_key().expression(),
            ApplicationDeliveryCredentials::secret_id().expression(),
            ApplicationDeliveryCredentials::secret_version().expression(),
            ApplicationDeliveryCredentials::generation().expression(),
            ApplicationDeliveryCredentials::status().expression(),
            ApplicationDeliveryCredentials::created_by().expression(),
            ApplicationDeliveryCredentials::created_at().expression(),
            ApplicationDeliveryCredentials::updated_at().expression(),
            ApplicationDeliveryCredentials::revoked_at().expression(),
        ]
    }
}

impl FromRow for DeliveryCredentialRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            id: decode(row, 3)?,
            audience: decode(row, 4)?,
            lookup_key: decode(row, 5)?,
            secret_id: decode(row, 6)?,
            secret_version: decode(row, 7)?,
            generation: decode(row, 8)?,
            status: decode(row, 9)?,
            created_by: decode(row, 10)?,
            created_at: decode(row, 11)?,
            updated_at: decode(row, 12)?,
            revoked_at: decode(row, 13)?,
        })
    }
}

impl DeliveryCredentialRow {
    fn credential(self) -> Result<ApplicationDeliveryCredential, RepositoryError> {
        let audience = parse_audience(&self.audience)?;
        let status = parse_status(&self.status)?;
        let secret =
            SecretVersionReference::new(SecretId::from_uuid(self.secret_id), self.secret_version)
                .map_err(stored)?;
        ApplicationDeliveryCredential::restore(
            OrganizationId::from_uuid(self.organization_id),
            ProjectId::from_uuid(self.project_id),
            ApplicationId::from_uuid(self.application_id),
            ApplicationDeliveryCredentialId::from_uuid(self.id),
            audience,
            self.lookup_key,
            secret,
            self.generation,
            status,
            PrincipalId::from_uuid(self.created_by),
            self.created_at,
            self.updated_at,
            self.revoked_at,
        )
        .map_err(stored)
    }
}

async fn insert_credential(
    transaction: &PostgresTransaction,
    credential: &ApplicationDeliveryCredential,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<ApplicationDeliveryCredentials>()
            .value(
                ApplicationDeliveryCredentials::organization_id(),
                credential.organization_id.as_uuid(),
            )
            .value(
                ApplicationDeliveryCredentials::project_id(),
                credential.project_id.as_uuid(),
            )
            .value(
                ApplicationDeliveryCredentials::application_id(),
                credential.application_id.as_uuid(),
            )
            .value(
                ApplicationDeliveryCredentials::id(),
                credential.id.as_uuid(),
            )
            .value(
                ApplicationDeliveryCredentials::audience(),
                audience_str(credential.audience),
            )
            .value(
                ApplicationDeliveryCredentials::lookup_key(),
                credential.lookup_key.as_str(),
            )
            .value(
                ApplicationDeliveryCredentials::secret_id(),
                credential.secret.secret_id.as_uuid(),
            )
            .value(
                ApplicationDeliveryCredentials::secret_version(),
                credential.secret.version,
            )
            .value(
                ApplicationDeliveryCredentials::generation(),
                credential.generation,
            )
            .value(
                ApplicationDeliveryCredentials::status(),
                status_str(credential.status),
            )
            .value(
                ApplicationDeliveryCredentials::created_by(),
                credential.created_by.as_uuid(),
            )
            .value(
                ApplicationDeliveryCredentials::created_at(),
                credential.created_at,
            )
            .value(
                ApplicationDeliveryCredentials::updated_at(),
                credential.updated_at,
            )
            .value(
                ApplicationDeliveryCredentials::revoked_at(),
                credential.revoked_at,
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application delivery credential", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application delivery credential identity or lookup key is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn update_credential_row(
    transaction: &PostgresTransaction,
    credential: &ApplicationDeliveryCredential,
    expected_generation: u64,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        update_table::<ApplicationDeliveryCredentials>()
            .set(
                ApplicationDeliveryCredentials::generation(),
                credential.generation,
            )
            .set(
                ApplicationDeliveryCredentials::status(),
                status_str(credential.status),
            )
            .set(
                ApplicationDeliveryCredentials::updated_at(),
                credential.updated_at,
            )
            .set(
                ApplicationDeliveryCredentials::revoked_at(),
                credential.revoked_at,
            )
            .filter(
                ApplicationDeliveryCredentials::organization_id()
                    .eq(credential.organization_id.as_uuid()),
            )
            .filter(
                ApplicationDeliveryCredentials::application_id()
                    .eq(credential.application_id.as_uuid()),
            )
            .filter(ApplicationDeliveryCredentials::id().eq(credential.id.as_uuid()))
            .filter(ApplicationDeliveryCredentials::generation().eq(expected_generation)),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("application delivery credential update", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Application delivery credential lookup key is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

fn audience_str(audience: ApplicationAudience) -> &'static str {
    match audience {
        ApplicationAudience::Anonymous => "anonymous",
        ApplicationAudience::ProjectMembers => "project_members",
        ApplicationAudience::AuthenticatedEndUsers => "authenticated_end_users",
    }
}

fn status_str(status: ApplicationDeliveryCredentialStatus) -> &'static str {
    match status {
        ApplicationDeliveryCredentialStatus::Active => "active",
        ApplicationDeliveryCredentialStatus::Disabled => "disabled",
        ApplicationDeliveryCredentialStatus::Revoked => "revoked",
    }
}

fn parse_audience(value: &str) -> Result<ApplicationAudience, RepositoryError> {
    match value {
        "anonymous" => Ok(ApplicationAudience::Anonymous),
        "project_members" => Ok(ApplicationAudience::ProjectMembers),
        "authenticated_end_users" => Ok(ApplicationAudience::AuthenticatedEndUsers),
        _ => Err(stored(format!("unknown audience {value}"))),
    }
}

fn parse_status(value: &str) -> Result<ApplicationDeliveryCredentialStatus, RepositoryError> {
    match value {
        "active" => Ok(ApplicationDeliveryCredentialStatus::Active),
        "disabled" => Ok(ApplicationDeliveryCredentialStatus::Disabled),
        "revoked" => Ok(ApplicationDeliveryCredentialStatus::Revoked),
        _ => Err(stored(format!("unknown status {value}"))),
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
        "stored Application delivery credential is invalid: {}",
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
    fn application_list_compiles_as_one_typed_scoped_query() {
        let compiled = select_from::<ApplicationDeliveryCredentials>()
            .select(DeliveryCredentialSelection)
            .filter(
                ApplicationDeliveryCredentials::organization_id()
                    .eq(OrganizationId::new().as_uuid()),
            )
            .filter(ApplicationDeliveryCredentials::project_id().eq(ProjectId::new().as_uuid()))
            .filter(
                ApplicationDeliveryCredentials::application_id().eq(ApplicationId::new().as_uuid()),
            )
            .order_by(
                ApplicationDeliveryCredentials::created_at(),
                OrderDirection::Asc,
            )
            .order_by(ApplicationDeliveryCredentials::id(), OrderDirection::Asc)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 3);
        assert!(compiled.sql.contains("\"organization_id\" = $1"));
        assert!(compiled.sql.contains("\"project_id\" = $2"));
        assert!(compiled.sql.contains("\"application_id\" = $3"));
        assert!(
            compiled
                .sql
                .contains("\"application_delivery_credentials\"")
        );
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}
