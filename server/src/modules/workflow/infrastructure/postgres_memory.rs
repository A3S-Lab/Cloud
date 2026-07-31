use std::collections::HashMap;

use a3s_memory::{MemoryItem, MemoryStore, MemoryType};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Row};

#[derive(Clone)]
pub struct PostgresMemoryStore {
    pool: PgPool,
}

impl PostgresMemoryStore {
    pub async fn connect(database_url: &str, max_connections: usize) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections as u32)
            .connect(database_url)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS workflow_memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL,
                importance REAL NOT NULL CHECK (importance >= 0 AND importance <= 1),
                tags TEXT[] NOT NULL DEFAULT '{}',
                memory_type TEXT NOT NULL,
                metadata JSONB NOT NULL DEFAULT '{}',
                access_count BIGINT NOT NULL DEFAULT 0 CHECK (access_count >= 0),
                last_accessed TIMESTAMPTZ
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS workflow_memories_recent_idx
                ON workflow_memories (timestamp DESC, id)
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS workflow_memories_tags_idx
                ON workflow_memories USING GIN (tags)
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn query_items(
        &self,
        query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
    ) -> anyhow::Result<Vec<MemoryItem>> {
        query
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(decode_row)
            .collect()
    }
}

#[async_trait]
impl MemoryStore for PostgresMemoryStore {
    async fn store(&self, item: MemoryItem) -> anyhow::Result<()> {
        let memory_type = serde_json::to_value(item.memory_type)?
            .as_str()
            .unwrap_or("episodic")
            .to_string();
        sqlx::query(
            r#"
            INSERT INTO workflow_memories (
                id, content, timestamp, importance, tags, memory_type,
                metadata, access_count, last_accessed
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE
            SET content = EXCLUDED.content,
                timestamp = EXCLUDED.timestamp,
                importance = EXCLUDED.importance,
                tags = EXCLUDED.tags,
                memory_type = EXCLUDED.memory_type,
                metadata = EXCLUDED.metadata,
                access_count = EXCLUDED.access_count,
                last_accessed = EXCLUDED.last_accessed
            "#,
        )
        .bind(&item.id)
        .bind(&item.content)
        .bind(item.timestamp)
        .bind(item.importance)
        .bind(&item.tags)
        .bind(memory_type)
        .bind(serde_json::to_value(&item.metadata)?)
        .bind(i64::from(item.access_count))
        .bind(item.last_accessed)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn retrieve(&self, id: &str) -> anyhow::Result<Option<MemoryItem>> {
        sqlx::query(
            r#"
            UPDATE workflow_memories
            SET access_count = access_count + 1, last_accessed = $2
            WHERE id = $1
            RETURNING id, content, timestamp, importance, tags, memory_type,
                      metadata, access_count, last_accessed
            "#,
        )
        .bind(id)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await?
        .map(decode_row)
        .transpose()
    }

    async fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryItem>> {
        self.query_items(
            sqlx::query(
                r#"
                SELECT id, content, timestamp, importance, tags, memory_type,
                       metadata, access_count, last_accessed
                FROM workflow_memories
                WHERE content ILIKE $1
                ORDER BY importance DESC, timestamp DESC, id
                LIMIT $2
                "#,
            )
            .bind(format!("%{}%", escape_like(query)))
            .bind(limit_value(limit)),
        )
        .await
    }

    async fn search_by_tags(
        &self,
        tags: &[String],
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryItem>> {
        self.query_items(
            sqlx::query(
                r#"
                SELECT id, content, timestamp, importance, tags, memory_type,
                       metadata, access_count, last_accessed
                FROM workflow_memories
                WHERE tags @> $1
                ORDER BY importance DESC, timestamp DESC, id
                LIMIT $2
                "#,
            )
            .bind(tags)
            .bind(limit_value(limit)),
        )
        .await
    }

    async fn get_recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryItem>> {
        self.query_items(
            sqlx::query(
                r#"
                SELECT id, content, timestamp, importance, tags, memory_type,
                       metadata, access_count, last_accessed
                FROM workflow_memories
                ORDER BY timestamp DESC, id
                LIMIT $1
                "#,
            )
            .bind(limit_value(limit)),
        )
        .await
    }

    async fn get_important(&self, threshold: f32, limit: usize) -> anyhow::Result<Vec<MemoryItem>> {
        self.query_items(
            sqlx::query(
                r#"
                SELECT id, content, timestamp, importance, tags, memory_type,
                       metadata, access_count, last_accessed
                FROM workflow_memories
                WHERE importance >= $1
                ORDER BY importance DESC, timestamp DESC, id
                LIMIT $2
                "#,
            )
            .bind(threshold.clamp(0.0, 1.0))
            .bind(limit_value(limit)),
        )
        .await
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM workflow_memories WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear(&self) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM workflow_memories")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn count(&self) -> anyhow::Result<usize> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM workflow_memories")
            .fetch_one(&self.pool)
            .await?;
        Ok(usize::try_from(count)?)
    }
}

fn decode_row(row: PgRow) -> anyhow::Result<MemoryItem> {
    let memory_type: String = row.get("memory_type");
    let memory_type: MemoryType = serde_json::from_value(serde_json::Value::String(memory_type))?;
    let content: String = row.get("content");
    let access_count: i64 = row.get("access_count");
    let metadata: serde_json::Value = row.get("metadata");
    Ok(MemoryItem {
        id: row.get("id"),
        content_lower: content.to_lowercase(),
        content,
        timestamp: row.get::<DateTime<Utc>, _>("timestamp"),
        importance: row.get("importance"),
        tags: row.get("tags"),
        memory_type,
        metadata: serde_json::from_value::<HashMap<String, String>>(metadata)?,
        access_count: u32::try_from(access_count)?,
        last_accessed: row.get("last_accessed"),
    })
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn limit_value(limit: usize) -> i64 {
    i64::try_from(limit.clamp(1, 500)).unwrap_or(500)
}
