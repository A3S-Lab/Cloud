use std::sync::Arc;

use a3s_runtime::contract::RuntimeObservation;
use a3s_workflow_protocol::NodeInvocation;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::modules::workflow::domain::{WorkflowError, WorkflowResult};

#[derive(Debug, Clone)]
pub struct PreparedNodeExecution {
    pub execution_id: String,
    pub token: String,
    pub invocation_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutionEvidence {
    pub execution_id: String,
    pub run_id: String,
    pub step_id: String,
    pub attempt: i32,
    pub node_id: String,
    pub provider_id: String,
    pub runtime_pool: Option<String>,
    pub unit_id: Option<String>,
    pub generation: Option<i64>,
    pub spec_digest: Option<String>,
    pub state: String,
    pub observation: Option<RuntimeObservation>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct PostgresNodeExecutionStore {
    pool: PgPool,
}

impl PostgresNodeExecutionStore {
    pub async fn connect(database_url: &str, max_connections: usize) -> WorkflowResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections as u32)
            .connect(database_url)
            .await
            .map_err(persistence)?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn migrate(&self) -> WorkflowResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS workflow_node_executions (
                execution_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                attempt INTEGER NOT NULL CHECK (attempt >= 1),
                node_id TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                runtime_pool TEXT,
                invocation JSONB NOT NULL,
                invocation_digest TEXT NOT NULL,
                access_token TEXT NOT NULL,
                unit_id TEXT,
                generation BIGINT,
                spec_digest TEXT,
                state TEXT NOT NULL,
                observation JSONB,
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                UNIQUE (run_id, step_id, attempt)
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(persistence)?;
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS workflow_node_executions_run_idx
                ON workflow_node_executions (run_id, created_at, execution_id)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(persistence)?;
        Ok(())
    }

    pub async fn prepare(
        &self,
        invocation: &NodeInvocation,
        provider_id: &str,
        runtime_pool: Option<&str>,
    ) -> WorkflowResult<PreparedNodeExecution> {
        let bytes = serde_json::to_vec(invocation)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        let invocation_digest = digest(&bytes);
        let invocation_json = serde_json::to_value(invocation)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        let now = Utc::now();
        let execution_id = Uuid::new_v4().to_string();
        let token = Uuid::new_v4().to_string();
        let row = sqlx::query(
            r#"
            INSERT INTO workflow_node_executions (
                execution_id, run_id, step_id, attempt, node_id, provider_id, runtime_pool,
                invocation, invocation_digest, access_token, state, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'prepared', $11, $11)
            ON CONFLICT (run_id, step_id, attempt) DO UPDATE
            SET updated_at = workflow_node_executions.updated_at
            RETURNING execution_id, access_token, invocation_digest
            "#,
        )
        .bind(execution_id)
        .bind(&invocation.run_id)
        .bind(&invocation.step_id)
        .bind(i32::try_from(invocation.attempt).map_err(|error| {
            WorkflowError::Persistence(format!("invalid node attempt: {error}"))
        })?)
        .bind(&invocation.node.id)
        .bind(provider_id)
        .bind(runtime_pool)
        .bind(invocation_json)
        .bind(&invocation_digest)
        .bind(token)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(persistence)?;
        let stored_digest: String = row.get("invocation_digest");
        if stored_digest != invocation_digest {
            return Err(WorkflowError::Conflict(format!(
                "step {} was redelivered with different invocation content",
                invocation.step_id
            )));
        }
        Ok(PreparedNodeExecution {
            execution_id: row.get("execution_id"),
            token: row.get("access_token"),
            invocation_digest,
        })
    }

    pub async fn mark_dispatched(
        &self,
        execution_id: &str,
        unit_id: &str,
        generation: u64,
        spec_digest: &str,
    ) -> WorkflowResult<()> {
        sqlx::query(
            r#"
            UPDATE workflow_node_executions
            SET unit_id = $2, generation = $3, spec_digest = $4,
                state = 'dispatched', updated_at = $5
            WHERE execution_id = $1
            "#,
        )
        .bind(execution_id)
        .bind(unit_id)
        .bind(i64::try_from(generation).map_err(|error| {
            WorkflowError::Persistence(format!("invalid runtime generation: {error}"))
        })?)
        .bind(spec_digest)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(persistence)?;
        Ok(())
    }

    pub async fn complete(
        &self,
        execution_id: &str,
        observation: &RuntimeObservation,
    ) -> WorkflowResult<()> {
        let state = format!("{:?}", observation.state).to_ascii_lowercase();
        let observation = serde_json::to_value(observation)
            .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
        sqlx::query(
            r#"
            UPDATE workflow_node_executions
            SET state = $2, observation = $3, updated_at = $4
            WHERE execution_id = $1
            "#,
        )
        .bind(execution_id)
        .bind(state)
        .bind(observation)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(persistence)?;
        Ok(())
    }

    pub async fn invocation(
        &self,
        execution_id: &str,
        token: &str,
    ) -> WorkflowResult<Option<NodeInvocation>> {
        let row = sqlx::query(
            r#"
            SELECT invocation
            FROM workflow_node_executions
            WHERE execution_id = $1 AND access_token = $2
            "#,
        )
        .bind(execution_id)
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map_err(persistence)?;
        row.map(|row| serde_json::from_value(row.get("invocation")))
            .transpose()
            .map_err(|error| WorkflowError::Persistence(error.to_string()))
    }

    pub async fn list_for_run(&self, run_id: &str) -> WorkflowResult<Vec<NodeExecutionEvidence>> {
        let rows = sqlx::query(
            r#"
            SELECT execution_id, run_id, step_id, attempt, node_id, provider_id, runtime_pool,
                   unit_id, generation, spec_digest, state, observation, created_at, updated_at
            FROM workflow_node_executions
            WHERE run_id = $1
            ORDER BY created_at, execution_id
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(persistence)?;
        rows.into_iter()
            .map(|row| {
                let observation = row
                    .get::<Option<serde_json::Value>, _>("observation")
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|error| WorkflowError::Persistence(error.to_string()))?;
                Ok(NodeExecutionEvidence {
                    execution_id: row.get("execution_id"),
                    run_id: row.get("run_id"),
                    step_id: row.get("step_id"),
                    attempt: row.get("attempt"),
                    node_id: row.get("node_id"),
                    provider_id: row.get("provider_id"),
                    runtime_pool: row.get("runtime_pool"),
                    unit_id: row.get("unit_id"),
                    generation: row.get("generation"),
                    spec_digest: row.get("spec_digest"),
                    state: row.get("state"),
                    observation,
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect()
    }
}

pub type SharedNodeExecutionStore = Arc<PostgresNodeExecutionStore>;

pub fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn persistence(error: sqlx::Error) -> WorkflowError {
    WorkflowError::Persistence(error.to_string())
}
