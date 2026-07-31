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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use a3s_runtime::contract::{RuntimeObservation, RuntimeUnitClass, RuntimeUnitState};
    use a3s_workflow_protocol::{
        NodeData, NodeExecutionPhase, NodeKind, NodeRuntimePolicy, NodeServiceContext, Position,
        WorkflowNode, NODE_INVOCATION_SCHEMA,
    };
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn test_database_url() -> Option<String> {
        std::env::var("A3S_WORKFLOW_TEST_DATABASE_URL").ok()
    }

    fn invocation(run_id: String) -> NodeInvocation {
        NodeInvocation {
            schema: NODE_INVOCATION_SCHEMA.to_string(),
            run_id,
            step_id: "agent-step".to_string(),
            attempt: 1,
            workflow_id: "workflow".to_string(),
            workflow_version: 1,
            phase: NodeExecutionPhase::Execute,
            node: WorkflowNode {
                id: "agent".to_string(),
                kind: NodeKind::Agent,
                position: Position { x: 0.0, y: 0.0 },
                data: NodeData {
                    label: "Agent".to_string(),
                    config: json!({"prompt": "ship it"}),
                    runtime: NodeRuntimePolicy::default(),
                },
            },
            workflow_input: json!({"task": "test persistence"}),
            dependencies: BTreeMap::new(),
            resume_payload: None,
            services: NodeServiceContext {
                gateway_base_url: "http://gateway.invalid".to_string(),
                default_model: "test-model".to_string(),
                memory_base_url: None,
                http_allowed_hosts: Vec::new(),
                max_http_response_bytes: 1024,
            },
        }
    }

    fn succeeded_observation(
        unit_id: &str,
        generation: u64,
        spec_digest: &str,
    ) -> RuntimeObservation {
        RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.to_string(),
            unit_id: unit_id.to_string(),
            generation,
            spec_digest: spec_digest.to_string(),
            class: RuntimeUnitClass::Task,
            state: RuntimeUnitState::Succeeded,
            provider_resource_id: Some("provider-resource".to_string()),
            provider_build: Some("test-provider".to_string()),
            observed_at_ms: 2,
            started_at_ms: Some(1),
            finished_at_ms: Some(2),
            health: None,
            outputs: Vec::new(),
            usage: None,
            evidence: None,
            provider_attestation: None,
            failure: None,
        }
    }

    #[test]
    fn creates_stable_sha256_digests() {
        assert_eq!(
            digest(b"a3s"),
            "sha256:d4b94a08ac87904968e9227901bcbad132561d65a2344495fcc041922ec7165e"
        );
    }

    #[tokio::test]
    async fn records_idempotent_runtime_dispatch_and_terminal_evidence() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let store = PostgresNodeExecutionStore::connect(&database_url, 2)
            .await
            .expect("connect execution store");
        let run_id = format!("node-store-test-{}", Uuid::new_v4());
        let invocation = invocation(run_id.clone());

        let prepared = store
            .prepare(&invocation, "production", Some("agents"))
            .await
            .expect("prepare execution");
        assert!(store
            .invocation(&prepared.execution_id, "wrong-token")
            .await
            .expect("wrong token lookup")
            .is_none());
        assert_eq!(
            store
                .invocation(&prepared.execution_id, &prepared.token)
                .await
                .expect("authorized invocation"),
            Some(invocation.clone())
        );

        let replay = store
            .prepare(&invocation, "ignored-on-replay", None)
            .await
            .expect("idempotent replay");
        assert_eq!(replay.execution_id, prepared.execution_id);
        assert_eq!(replay.token, prepared.token);
        assert_eq!(replay.invocation_digest, prepared.invocation_digest);

        let mut changed = invocation.clone();
        changed.workflow_input = json!({"task": "changed"});
        assert!(matches!(
            store.prepare(&changed, "production", Some("agents")).await,
            Err(WorkflowError::Conflict(_))
        ));

        assert!(matches!(
            store
                .mark_dispatched(&prepared.execution_id, "unit-agent", u64::MAX, "sha256:bad")
                .await,
            Err(WorkflowError::Persistence(_))
        ));
        let spec_digest = format!("sha256:{}", "a".repeat(64));
        store
            .mark_dispatched(&prepared.execution_id, "unit-agent", 7, &spec_digest)
            .await
            .expect("mark dispatched");
        let dispatched = store
            .list_for_run(&run_id)
            .await
            .expect("dispatch evidence");
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].provider_id, "production");
        assert_eq!(dispatched[0].runtime_pool.as_deref(), Some("agents"));
        assert_eq!(dispatched[0].state, "dispatched");
        assert_eq!(dispatched[0].unit_id.as_deref(), Some("unit-agent"));
        assert_eq!(dispatched[0].generation, Some(7));
        assert_eq!(
            dispatched[0].spec_digest.as_deref(),
            Some(spec_digest.as_str())
        );

        let observation = succeeded_observation("unit-agent", 7, &spec_digest);
        store
            .complete(&prepared.execution_id, &observation)
            .await
            .expect("complete execution");
        let completed = store
            .list_for_run(&run_id)
            .await
            .expect("terminal evidence");
        assert_eq!(completed[0].state, "succeeded");
        assert_eq!(completed[0].observation.as_ref(), Some(&observation));

        sqlx::query("DELETE FROM workflow_node_executions WHERE run_id = $1")
            .bind(&run_id)
            .execute(store.pool())
            .await
            .expect("cleanup execution evidence");
        assert!(store
            .list_for_run(&run_id)
            .await
            .expect("empty evidence")
            .is_empty());
    }
}
