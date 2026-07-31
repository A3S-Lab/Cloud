use std::sync::Arc;

use a3s_orm::{
    delete_from, insert_into, orm_table, select_from, sql_query, update_table, Database, Migration,
    Migrator, OrderDirection, PostgresDialect, PostgresExecutor,
};
use async_trait::async_trait;
use serde_json::Value;

use crate::modules::workflow::domain::{
    WorkflowDefinition, WorkflowError, WorkflowRepository, WorkflowResult,
};

orm_table! {
    struct WorkflowRecord => "workflow_definitions" {
        id: String => "id",
        name: String => "name",
        description: String => "description",
        version: i64 => "version",
        definition: Value => "definition",
        created_at: String => "created_at",
        updated_at: String => "updated_at",
    }
}

type WorkflowDatabase = Database<PostgresDialect, PostgresExecutor>;

#[derive(Clone)]
pub struct PostgresWorkflowRepository {
    database: Arc<WorkflowDatabase>,
}

impl PostgresWorkflowRepository {
    pub async fn connect(database_url: &str, max_connections: usize) -> WorkflowResult<Self> {
        let executor = PostgresExecutor::connect_no_tls(database_url, max_connections)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        Migrator::new(executor.clone())
            .run([Migration::new(
                "001",
                "create workflow definitions",
                r#"
                CREATE TABLE workflow_definitions (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    description TEXT NOT NULL,
                    version BIGINT NOT NULL CHECK (version >= 1),
                    definition JSONB NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX workflow_definitions_updated_at_idx
                    ON workflow_definitions (updated_at DESC);
                "#,
            )])
            .await
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;

        Ok(Self {
            database: Arc::new(Database::new(PostgresDialect, executor)),
        })
    }

    pub async fn health_check(&self) -> WorkflowResult<()> {
        self.database
            .fetch_one_as(sql_query::<i64>("SELECT 1"))
            .await
            .map(|_| ())
            .map_err(|error| WorkflowError::Persistence(error.to_string()))
    }

    fn decode(value: Value) -> WorkflowResult<WorkflowDefinition> {
        serde_json::from_value(value).map_err(|error| {
            WorkflowError::Persistence(format!("invalid stored workflow: {error}"))
        })
    }
}

#[async_trait]
impl WorkflowRepository for PostgresWorkflowRepository {
    async fn list(&self) -> WorkflowResult<Vec<WorkflowDefinition>> {
        self.database
            .fetch_all_as(
                select_from::<WorkflowRecord>()
                    .select(WorkflowRecord::definition())
                    .order_by(WorkflowRecord::updated_at(), OrderDirection::Desc),
            )
            .await
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?
            .rows
            .into_iter()
            .map(Self::decode)
            .collect()
    }

    async fn find(&self, id: &str) -> WorkflowResult<Option<WorkflowDefinition>> {
        self.database
            .fetch_optional_as(
                select_from::<WorkflowRecord>()
                    .select(WorkflowRecord::definition())
                    .filter(WorkflowRecord::id().eq(id)),
            )
            .await
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?
            .map(Self::decode)
            .transpose()
    }

    async fn create(&self, workflow: &WorkflowDefinition) -> WorkflowResult<()> {
        let version = i64::try_from(workflow.version)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        let definition = serde_json::to_value(workflow)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        self.database
            .execute(
                insert_into::<WorkflowRecord>()
                    .value(WorkflowRecord::id(), workflow.id.clone())
                    .value(WorkflowRecord::name(), workflow.name.clone())
                    .value(WorkflowRecord::description(), workflow.description.clone())
                    .value(WorkflowRecord::version(), version)
                    .value(WorkflowRecord::definition(), definition)
                    .value(
                        WorkflowRecord::created_at(),
                        workflow.created_at.to_rfc3339(),
                    )
                    .value(
                        WorkflowRecord::updated_at(),
                        workflow.updated_at.to_rfc3339(),
                    ),
            )
            .await
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        Ok(())
    }

    async fn update(
        &self,
        workflow: &WorkflowDefinition,
        expected_version: u64,
    ) -> WorkflowResult<()> {
        let version = i64::try_from(workflow.version)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        let expected_version = i64::try_from(expected_version)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        let definition = serde_json::to_value(workflow)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        let result = self
            .database
            .execute(
                update_table::<WorkflowRecord>()
                    .set(WorkflowRecord::name(), workflow.name.clone())
                    .set(WorkflowRecord::description(), workflow.description.clone())
                    .set(WorkflowRecord::version(), version)
                    .set(WorkflowRecord::definition(), definition)
                    .set(
                        WorkflowRecord::updated_at(),
                        workflow.updated_at.to_rfc3339(),
                    )
                    .filter(WorkflowRecord::id().eq(workflow.id.as_str()))
                    .filter(WorkflowRecord::version().eq(expected_version)),
            )
            .await
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        if result.rows_affected == 0 {
            return Err(WorkflowError::Conflict(format!(
                "{} changed after version {}",
                workflow.id, expected_version
            )));
        }
        Ok(())
    }

    async fn delete(&self, id: &str) -> WorkflowResult<bool> {
        let result = self
            .database
            .execute(delete_from::<WorkflowRecord>().filter(WorkflowRecord::id().eq(id)))
            .await
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        Ok(result.rows_affected > 0)
    }
}
