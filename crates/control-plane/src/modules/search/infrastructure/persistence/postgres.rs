use super::postgres_schema::AuthorizedSearchProjections;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::search::domain::{
    ISearchRepository, SearchQuery, SearchResourceKind, SearchResult,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
use a3s_orm::expression::{Expression, Selection};
use a3s_orm::function::{bound, sql_function};
use a3s_orm::{
    select_from, Database, DecodeError, FromRow, FromValue, OrderDirection, PostgresDialect,
    PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

struct SearchRow {
    organization_id: Uuid,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    workload_id: Option<Uuid>,
    resource_kind: String,
    id: Uuid,
    title: String,
    description: String,
    state: Option<String>,
    updated_at: DateTime<Utc>,
}

struct SearchSelection;

impl Selection for SearchSelection {
    type Output = SearchRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AuthorizedSearchProjections::organization_id().expression(),
            AuthorizedSearchProjections::project_id().expression(),
            AuthorizedSearchProjections::environment_id().expression(),
            AuthorizedSearchProjections::workload_id().expression(),
            AuthorizedSearchProjections::resource_kind().expression(),
            AuthorizedSearchProjections::resource_id().expression(),
            AuthorizedSearchProjections::title().expression(),
            AuthorizedSearchProjections::description().expression(),
            AuthorizedSearchProjections::state().expression(),
            AuthorizedSearchProjections::updated_at().expression(),
        ]
    }
}

impl FromRow for SearchRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            workload_id: decode(row, 3)?,
            resource_kind: decode(row, 4)?,
            id: decode(row, 5)?,
            title: decode(row, 6)?,
            description: decode(row, 7)?,
            state: decode(row, 8)?,
            updated_at: decode(row, 9)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresSearchRepository {
    executor: PostgresExecutor,
}

impl PostgresSearchRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }

    async fn fetch_rank(
        &self,
        organization_id: OrganizationId,
        predicate: Expression,
        limit: u16,
        resource_access: &ResourceAccessEvaluator,
    ) -> Result<Vec<SearchResult>, RepositoryError> {
        let rows = Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<AuthorizedSearchProjections>()
                    .select(SearchSelection)
                    .filter(
                        AuthorizedSearchProjections::organization_id()
                            .eq(organization_id.as_uuid()),
                    )
                    .filter(predicate)
                    .filter(resource_visibility_predicate(resource_access))
                    .order_by(
                        AuthorizedSearchProjections::resource_kind(),
                        OrderDirection::Asc,
                    )
                    .order_by(
                        AuthorizedSearchProjections::title_key(),
                        OrderDirection::Asc,
                    )
                    .order_by(
                        AuthorizedSearchProjections::resource_id_text(),
                        OrderDirection::Asc,
                    )
                    .limit(u64::from(limit)),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows;
        rows.into_iter().map(decode_row).collect()
    }
}

#[async_trait]
impl ISearchRepository for PostgresSearchRepository {
    async fn search(
        &self,
        organization_id: OrganizationId,
        query: &SearchQuery,
        limit: u16,
        resource_access: &ResourceAccessEvaluator,
    ) -> Result<Vec<SearchResult>, RepositoryError> {
        let query_text = query.as_str().to_owned();
        let exact = AuthorizedSearchProjections::title_key()
            .eq(query_text.clone())
            .or(AuthorizedSearchProjections::resource_id_text().eq(query_text.clone()));
        let prefix = sql_function::<bool>(
            "starts_with",
            [
                AuthorizedSearchProjections::title_key().expression(),
                bound::<String>(query_text.clone()).expression(),
            ],
        )
        .eq(true);
        let contains = sql_function::<i32>(
            "strpos",
            [
                AuthorizedSearchProjections::search_text().expression(),
                bound::<String>(query_text).expression(),
            ],
        )
        .gt(0);

        let exact = self
            .fetch_rank(organization_id, exact, limit, resource_access)
            .await?;
        let prefix = self
            .fetch_rank(organization_id, prefix, limit, resource_access)
            .await?;
        let contains = self
            .fetch_rank(organization_id, contains, limit, resource_access)
            .await?;
        let mut seen = BTreeSet::new();
        let mut results = Vec::with_capacity(usize::from(limit));
        for result in exact.into_iter().chain(prefix).chain(contains) {
            if seen.insert((result.kind, result.id)) {
                results.push(result);
                if results.len() == usize::from(limit) {
                    break;
                }
            }
        }
        Ok(results)
    }
}

fn resource_visibility_predicate(resource_access: &ResourceAccessEvaluator) -> Expression {
    if resource_access.is_organization_wide() {
        return AuthorizedSearchProjections::organization_id()
            .eq_column(AuthorizedSearchProjections::organization_id());
    }
    let mut predicates = resource_access.granted_scopes().map(|scope| match scope {
        ResourceGrantScope::Project { project_id } => AuthorizedSearchProjections::project_id()
            .eq(Some(project_id.as_uuid()))
            .and(AuthorizedSearchProjections::resource_kind().ne("node".to_owned())),
        ResourceGrantScope::Environment {
            project_id,
            environment_id,
        } => AuthorizedSearchProjections::project_id()
            .eq(Some(project_id.as_uuid()))
            .and(AuthorizedSearchProjections::environment_id().eq(Some(environment_id.as_uuid())))
            .and(AuthorizedSearchProjections::resource_kind().ne("node".to_owned())),
        ResourceGrantScope::Node { node_id } => AuthorizedSearchProjections::resource_kind()
            .eq("node".to_owned())
            .and(AuthorizedSearchProjections::resource_id().eq(node_id.as_uuid())),
    });
    let Some(first) = predicates.next() else {
        return AuthorizedSearchProjections::organization_id()
            .ne_column(AuthorizedSearchProjections::organization_id());
    };
    predicates.fold(first, Expression::or)
}

fn decode_row(row: SearchRow) -> Result<SearchResult, RepositoryError> {
    let SearchRow {
        organization_id,
        project_id,
        environment_id,
        workload_id,
        resource_kind,
        id,
        title,
        description,
        state,
        updated_at,
    } = row;
    let result = SearchResult {
        organization_id: OrganizationId::from_uuid(organization_id),
        project_id,
        environment_id,
        workload_id,
        kind: SearchResourceKind::parse(&resource_kind).map_err(RepositoryError::Storage)?,
        id,
        title,
        description,
        state,
        updated_at,
    };
    result.validate().map_err(RepositoryError::Storage)?;
    Ok(result)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
