use super::context;
use a3s_boot::CommandHandler;
use a3s_cloud_contracts::{NodeLogChunkBatch, NodeLogChunkReport};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::domain::services::{
    ILogChunkStore, LogChunkStoreError, RetrievedLogChunk, StoredLogChunk,
};
use a3s_cloud_control_plane::modules::fleet::{
    LocalLogChunkStore, PostgresNodeRepository, RecordNodeLogChunks, RecordNodeLogChunksHandler,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::NodeId;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{RuntimeLogQuery, RuntimeLogStream, RuntimeUnitSpec};
use a3s_runtime::RuntimeClient;
use async_trait::async_trait;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const LOG_OBJECT_PUBLISH_CRASH_EXIT_CODE: i32 = 86;
const LOG_OBJECT_PUBLISH_CRASH_PROBE_ENV: &str = "A3S_CLOUD_LOG_OBJECT_PUBLISH_CRASH_PROBE";
const LOG_OBJECT_PUBLISH_CRASH_POSTGRES_ENV: &str =
    "A3S_CLOUD_LOG_OBJECT_PUBLISH_CRASH_POSTGRES_URL";
const LOG_OBJECT_PUBLISH_CRASH_ROOT_ENV: &str = "A3S_CLOUD_LOG_OBJECT_PUBLISH_CRASH_ROOT";
const LOG_OBJECT_PUBLISH_CRASH_NODE_ENV: &str = "A3S_CLOUD_LOG_OBJECT_PUBLISH_CRASH_NODE_ID";
const LOG_OBJECT_PUBLISH_CRASH_BATCH_ENV: &str = "A3S_CLOUD_LOG_OBJECT_PUBLISH_CRASH_BATCH";
const LOG_OBJECT_PUBLISH_CRASH_MARKER_ENV: &str = "A3S_CLOUD_LOG_OBJECT_PUBLISH_CRASH_MARKER";
const LOG_OBJECT_PUBLISH_CRASH_ORDINAL_ENV: &str = "A3S_CLOUD_LOG_OBJECT_PUBLISH_CRASH_ORDINAL";
const LOG_OBJECT_PUBLISH_CRASH_TEST: &str = "log_object_publish_crash_probe";
const LOG_RECOVERY_PROBE_MARKER: &str = "log-recovery-probe";

#[derive(Clone)]
pub struct LogRecoveryFixture {
    pub corrupted_sequence: u64,
    pub corrupted_stream: &'static str,
}

pub(super) async fn persist_redacted_docker_logs(
    postgres_url: &str,
    executor: &PostgresExecutor,
    node_id: NodeId,
    runtime: Arc<dyn RuntimeClient>,
    spec: &RuntimeUnitSpec,
    security_state_dir: &Path,
    sensitive_plaintexts: &[&str],
) -> Result<LogRecoveryFixture, Box<dyn std::error::Error>> {
    assert_eq!(
        spec.secrets
            .iter()
            .filter(|secret| !matches!(
                secret.target,
                a3s_runtime::contract::SecretTarget::RegistryCredential
            ))
            .count(),
        2
    );
    let query = RuntimeLogQuery {
        schema: RuntimeLogQuery::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        cursor: None,
        limit: 32,
        stream: None,
    };
    let mut chunks = Vec::new();
    for attempt in 0..20 {
        chunks = runtime.logs(&query).await?;
        let stdout_ready = chunks.iter().any(|chunk| {
            chunk.stream == RuntimeLogStream::Stdout && chunk.data.contains("env-secret=[REDACTED]")
        });
        let stderr_ready = chunks.iter().any(|chunk| {
            chunk.stream == RuntimeLogStream::Stderr
                && chunk.data.contains("file-secret=[REDACTED]")
        });
        let recovery_probe_ready = chunks
            .iter()
            .any(|chunk| chunk.data.trim_end() == LOG_RECOVERY_PROBE_MARKER);
        if stdout_ready && stderr_ready && recovery_probe_ready {
            break;
        }
        if attempt < 19 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    if chunks.iter().any(|chunk| {
        sensitive_plaintexts
            .iter()
            .any(|plaintext| chunk.data.contains(plaintext))
    }) || !chunks.iter().any(|chunk| {
        chunk.stream == RuntimeLogStream::Stdout && chunk.data.contains("env-secret=[REDACTED]")
    }) || !chunks.iter().any(|chunk| {
        chunk.stream == RuntimeLogStream::Stderr && chunk.data.contains("file-secret=[REDACTED]")
    }) || !chunks
        .iter()
        .any(|chunk| chunk.data.trim_end() == LOG_RECOVERY_PROBE_MARKER)
    {
        return Err(
            std::io::Error::other("real Docker Secret logs were not completely redacted").into(),
        );
    }
    let crash_ordinal = chunks
        .iter()
        .position(|chunk| chunk.data.trim_end() == LOG_RECOVERY_PROBE_MARKER)
        .ok_or("real Docker logs omitted the recovery probe marker")?;

    let batch = NodeLogChunkBatch {
        schema: NodeLogChunkBatch::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        sent_at: Utc::now(),
        chunks: chunks
            .into_iter()
            .map(|chunk| NodeLogChunkReport {
                unit_id: spec.unit_id.clone(),
                generation: spec.generation,
                checksum: format!("sha256:{:x}", Sha256::digest(chunk.data.as_bytes())),
                chunk,
            })
            .collect(),
        gaps: Vec::new(),
    };
    batch.validate()?;

    let crashed_object_key = crash_after_log_object_publish(
        postgres_url,
        executor,
        security_state_dir,
        node_id,
        &batch,
        crash_ordinal,
    )
    .await?;
    let first = record_log_batch(executor, security_state_dir, node_id, batch.clone()).await?;
    assert!(!first.replayed);
    assert_eq!(usize::from(first.accepted_chunks), batch.chunks.len());
    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_log_batch_chunks where batch_id = ",)
                    .bind(batch.batch_id),
            )
            .await?,
        i64::try_from(batch.chunks.len())?,
        "control-plane recovery did not adopt each exact object once"
    );

    // Recreate the handler, repository, and object-store adapter to model a
    // control-plane restart after the batch receipt became durable.
    let replay = record_log_batch(executor, security_state_dir, node_id, batch.clone()).await?;
    assert!(replay.replayed);
    assert_eq!(replay.accepted_chunks, first.accepted_chunks);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_log_batch_chunks where batch_id = ",)
                    .bind(batch.batch_id),
            )
            .await?,
        i64::try_from(batch.chunks.len())?,
        "control-plane receipt replay duplicated log batch membership"
    );
    assert_log_objects_redacted(&security_state_dir.join("logs"), sensitive_plaintexts)?;

    let corrupted = batch
        .chunks
        .get(crash_ordinal)
        .cloned()
        .ok_or("log recovery probe ordinal exceeded its batch")?;
    let objects = LocalLogChunkStore::new(security_state_dir.join("logs"))?;
    assert_eq!(
        objects
            .get(&crashed_object_key, &corrupted.checksum)
            .await?,
        RetrievedLogChunk::Found(corrupted.clone())
    );
    overwrite_with_corrupt_log_object(&security_state_dir.join("logs").join(&crashed_object_key))?;
    assert_eq!(
        objects
            .get(&crashed_object_key, &corrupted.checksum)
            .await?,
        RetrievedLogChunk::Corrupt
    );

    // A receipt replay after corruption must not silently repair or replace an
    // already accepted immutable object. The authoritative query path exposes
    // the exact sequence as a typed corrupt gap.
    let replay_after_corruption =
        record_log_batch(executor, security_state_dir, node_id, batch).await?;
    assert!(replay_after_corruption.replayed);
    assert_eq!(
        replay_after_corruption.accepted_chunks,
        first.accepted_chunks
    );
    assert_eq!(
        objects
            .get(&crashed_object_key, &corrupted.checksum)
            .await?,
        RetrievedLogChunk::Corrupt
    );

    Ok(LogRecoveryFixture {
        corrupted_sequence: corrupted.chunk.sequence,
        corrupted_stream: match corrupted.chunk.stream {
            RuntimeLogStream::Stdout => "stdout",
            RuntimeLogStream::Stderr => "stderr",
        },
    })
}

async fn crash_after_log_object_publish(
    postgres_url: &str,
    executor: &PostgresExecutor,
    security_state_dir: &Path,
    node_id: NodeId,
    batch: &NodeLogChunkBatch,
    crash_ordinal: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let crash_ordinal = u16::try_from(crash_ordinal)?;
    let probe_id = Uuid::now_v7();
    let batch_path = security_state_dir.join(format!(".log-crash-batch-{probe_id}.json"));
    let marker_path = security_state_dir.join(format!(".log-crash-marker-{probe_id}"));
    write_durable_file(&batch_path, &serde_json::to_vec(batch)?)?;

    let executable = std::env::current_exe()?;
    let postgres_url = postgres_url.to_owned();
    let log_root = security_state_dir.join("logs");
    let node_id = node_id.to_string();
    let child_batch_path = batch_path.clone();
    let child_marker_path = marker_path.clone();
    let status = tokio::task::spawn_blocking(move || {
        Command::new(executable)
            .arg(LOG_OBJECT_PUBLISH_CRASH_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(LOG_OBJECT_PUBLISH_CRASH_PROBE_ENV, "1")
            .env(LOG_OBJECT_PUBLISH_CRASH_POSTGRES_ENV, postgres_url)
            .env(LOG_OBJECT_PUBLISH_CRASH_ROOT_ENV, log_root)
            .env(LOG_OBJECT_PUBLISH_CRASH_NODE_ENV, node_id)
            .env(LOG_OBJECT_PUBLISH_CRASH_BATCH_ENV, child_batch_path)
            .env(LOG_OBJECT_PUBLISH_CRASH_MARKER_ENV, child_marker_path)
            .env(
                LOG_OBJECT_PUBLISH_CRASH_ORDINAL_ENV,
                crash_ordinal.to_string(),
            )
            .status()
    })
    .await??;
    if status.code() != Some(LOG_OBJECT_PUBLISH_CRASH_EXIT_CODE) {
        return Err(std::io::Error::other(format!(
            "log object crash probe exited with {status} instead of code {LOG_OBJECT_PUBLISH_CRASH_EXIT_CODE}"
        ))
        .into());
    }

    let object_key = std::fs::read_to_string(&marker_path)?;
    std::fs::remove_file(&batch_path)?;
    std::fs::remove_file(&marker_path)?;
    if object_key.is_empty() || !security_state_dir.join("logs").join(&object_key).is_file() {
        return Err(
            std::io::Error::other("crash probe did not durably publish its log object").into(),
        );
    }

    let database = Database::new(PostgresDialect, executor.clone());
    let batch_rows = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from node_log_batches where batch_id = ")
                .bind(batch.batch_id),
        )
        .await?;
    let chunk_rows = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from node_log_chunks where object_key = ")
                .bind(object_key.clone()),
        )
        .await?;
    assert_eq!(
        (batch_rows, chunk_rows),
        (0, 0),
        "control-plane crash probe committed log metadata after process death"
    );

    let report = batch
        .chunks
        .get(usize::from(crash_ordinal))
        .ok_or("crash probe ordinal exceeded its batch")?;
    let objects = LocalLogChunkStore::new(security_state_dir.join("logs"))?;
    assert_eq!(
        objects.get(&object_key, &report.checksum).await?,
        RetrievedLogChunk::Found(report.clone()),
        "crash probe did not leave a complete immutable object before receipt persistence"
    );
    Ok(object_key)
}

pub async fn run_log_object_publish_crash_probe() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var(LOG_OBJECT_PUBLISH_CRASH_PROBE_ENV).as_deref() != Ok("1") {
        return Err(std::io::Error::other(
            "log object crash probe requires its private parent-process marker",
        )
        .into());
    }
    let postgres_url = required_probe_environment(LOG_OBJECT_PUBLISH_CRASH_POSTGRES_ENV)?;
    let log_root = PathBuf::from(required_probe_environment(
        LOG_OBJECT_PUBLISH_CRASH_ROOT_ENV,
    )?);
    let node_id = NodeId::from_uuid(Uuid::parse_str(&required_probe_environment(
        LOG_OBJECT_PUBLISH_CRASH_NODE_ENV,
    )?)?);
    let batch_path = PathBuf::from(required_probe_environment(
        LOG_OBJECT_PUBLISH_CRASH_BATCH_ENV,
    )?);
    let marker_path = PathBuf::from(required_probe_environment(
        LOG_OBJECT_PUBLISH_CRASH_MARKER_ENV,
    )?);
    let crash_ordinal =
        required_probe_environment(LOG_OBJECT_PUBLISH_CRASH_ORDINAL_ENV)?.parse::<u16>()?;
    let batch = serde_json::from_slice::<NodeLogChunkBatch>(&std::fs::read(batch_path)?)?;
    let executor = PostgresExecutor::connect_no_tls(&postgres_url, 2)?;
    let nodes: Arc<dyn INodeControlRepository> = Arc::new(PostgresNodeRepository::new(executor));
    let inner = LocalLogChunkStore::new(log_root)?;
    let objects: Arc<dyn ILogChunkStore> = Arc::new(CrashAfterLogObjectPublish {
        inner,
        crash_ordinal,
        marker_path,
    });

    let result = RecordNodeLogChunksHandler::new(nodes, objects)
        .execute(
            RecordNodeLogChunks {
                authenticated_node_id: node_id,
                batch,
                received_at: Utc::now(),
            },
            context(),
        )
        .await?;
    Err(std::io::Error::other(format!(
        "log object crash probe returned without terminating the process: {result:?}"
    ))
    .into())
}

struct CrashAfterLogObjectPublish {
    inner: LocalLogChunkStore,
    crash_ordinal: u16,
    marker_path: PathBuf,
}

#[async_trait]
impl ILogChunkStore for CrashAfterLogObjectPublish {
    async fn put(
        &self,
        batch_id: Uuid,
        node_id: Uuid,
        ordinal: u16,
        report: &NodeLogChunkReport,
    ) -> Result<StoredLogChunk, LogChunkStoreError> {
        let stored = self.inner.put(batch_id, node_id, ordinal, report).await?;
        if ordinal == self.crash_ordinal {
            write_durable_file(&self.marker_path, stored.object_key.as_bytes()).map_err(
                |error| {
                    LogChunkStoreError::Unavailable(format!(
                        "write log object crash marker: {error}"
                    ))
                },
            )?;
            std::process::exit(LOG_OBJECT_PUBLISH_CRASH_EXIT_CODE);
        }
        Ok(stored)
    }

    async fn get(
        &self,
        object_key: &str,
        expected_checksum: &str,
    ) -> Result<RetrievedLogChunk, LogChunkStoreError> {
        self.inner.get(object_key, expected_checksum).await
    }

    async fn remove(&self, object_key: &str) -> Result<(), LogChunkStoreError> {
        self.inner.remove(object_key).await
    }

    async fn health(&self) -> Result<bool, LogChunkStoreError> {
        self.inner.health().await
    }
}

async fn record_log_batch(
    executor: &PostgresExecutor,
    security_state_dir: &Path,
    node_id: NodeId,
    batch: NodeLogChunkBatch,
) -> Result<a3s_cloud_contracts::NodeLogChunkReceipt, Box<dyn std::error::Error>> {
    let nodes: Arc<dyn INodeControlRepository> =
        Arc::new(PostgresNodeRepository::new(executor.clone()));
    let objects: Arc<dyn ILogChunkStore> =
        Arc::new(LocalLogChunkStore::new(security_state_dir.join("logs"))?);
    RecordNodeLogChunksHandler::new(nodes, objects)
        .execute(
            RecordNodeLogChunks {
                authenticated_node_id: node_id,
                batch,
                received_at: Utc::now(),
            },
            context(),
        )
        .await?
        .map_err(|error| {
            std::io::Error::other(format!("could not persist real Docker log batch: {error}"))
                .into()
        })
}

fn assert_log_objects_redacted(
    root: &Path,
    sensitive_plaintexts: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let marker = b"[REDACTED]";
    let mut directories = vec![root.to_path_buf()];
    let mut found_marker = false;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                let body = std::fs::read(entry.path())?;
                if sensitive_plaintexts.iter().any(|plaintext| {
                    let secret = plaintext.as_bytes();
                    !secret.is_empty() && body.windows(secret.len()).any(|window| window == secret)
                }) {
                    return Err(std::io::Error::other(
                        "plaintext Secret reached the durable log object store",
                    )
                    .into());
                }
                found_marker |= body.windows(marker.len()).any(|window| window == marker);
            }
        }
    }
    if !found_marker {
        return Err(
            std::io::Error::other("durable log objects contain no redaction evidence").into(),
        );
    }
    Ok(())
}

fn required_probe_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("log object crash probe omitted {name}")))
}

fn write_durable_file(path: &Path, body: &[u8]) -> Result<(), std::io::Error> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(body)?;
    file.sync_all()?;
    sync_parent_directory(path)
}

fn overwrite_with_corrupt_log_object(path: &Path) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(b"{\"corrupt\":true}")?;
    file.sync_all()?;
    sync_parent_directory(path)
}

fn sync_parent_directory(path: &Path) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("durable test file has no parent"))?;
    std::fs::File::open(parent)?.sync_all()
}
