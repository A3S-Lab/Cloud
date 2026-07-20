use super::*;
use a3s_runtime::contract::RuntimeLogStream;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogChunkRow {
    unit_id: String,
    generation: u64,
    cursor: String,
    sequence: u64,
    observed_at_ms: u64,
    stream: String,
    checksum: String,
    object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogChunkMetadataRow {
    unit_id: String,
    generation: u64,
    cursor: String,
    sequence: u64,
    observed_at_ms: u64,
    stream: String,
    checksum: String,
    object_key: String,
    retained_at: Option<DateTime<Utc>>,
}

impl FromRow for LogChunkMetadataRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            unit_id: decode(row, 0)?,
            generation: decode(row, 1)?,
            cursor: decode(row, 2)?,
            sequence: decode(row, 3)?,
            observed_at_ms: decode(row, 4)?,
            stream: decode(row, 5)?,
            checksum: decode(row, 6)?,
            object_key: decode(row, 7)?,
            retained_at: decode(row, 8)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogRetentionRow {
    node_id: Uuid,
    unit_id: String,
    generation: u64,
    sequence: u64,
    object_key: String,
    received_at: DateTime<Utc>,
}

impl FromRow for LogRetentionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            node_id: decode(row, 0)?,
            unit_id: decode(row, 1)?,
            generation: decode(row, 2)?,
            sequence: decode(row, 3)?,
            object_key: decode(row, 4)?,
            received_at: decode(row, 5)?,
        })
    }
}

impl FromRow for LogChunkRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            unit_id: decode(row, 0)?,
            generation: decode(row, 1)?,
            cursor: decode(row, 2)?,
            sequence: decode(row, 3)?,
            observed_at_ms: decode(row, 4)?,
            stream: decode(row, 5)?,
            checksum: decode(row, 6)?,
            object_key: decode(row, 7)?,
        })
    }
}

impl From<&NodeLogChunkReceiptDraft> for LogChunkRow {
    fn from(value: &NodeLogChunkReceiptDraft) -> Self {
        Self {
            unit_id: value.unit_id.clone(),
            generation: value.generation,
            cursor: value.cursor.clone(),
            sequence: value.sequence,
            observed_at_ms: value.observed_at_ms,
            stream: value.stream.clone(),
            checksum: value.checksum.clone(),
            object_key: value.object_key.clone(),
        }
    }
}

impl LogChunkMetadataRow {
    fn metadata(self, node_id: NodeId) -> Result<NodeLogChunkMetadata, PostgresPersistenceError> {
        NodeLogChunkReceiptDraft {
            unit_id: self.unit_id,
            generation: self.generation,
            cursor: self.cursor,
            sequence: self.sequence,
            observed_at_ms: self.observed_at_ms,
            stream: self.stream,
            checksum: self.checksum,
            object_key: self.object_key,
        }
        .metadata(node_id, self.retained_at)
        .map_err(PostgresPersistenceError::Invariant)
    }
}

impl LogRetentionRow {
    fn target(self) -> Result<NodeLogRetentionTarget, PostgresPersistenceError> {
        let target = NodeLogRetentionTarget {
            node_id: NodeId::from_uuid(self.node_id),
            unit_id: self.unit_id,
            generation: self.generation,
            sequence: self.sequence,
            object_key: self.object_key,
            received_at: self.received_at,
        };
        target
            .validate()
            .map_err(PostgresPersistenceError::Invariant)?;
        Ok(target)
    }
}

pub(in super::super) async fn record_gateway_acknowledgement(
    executor: &PostgresExecutor,
    mut acknowledgement: NodeGatewayAck,
    received_at: DateTime<Utc>,
) -> Result<NodeGatewayAckReceipt, RepositoryError> {
    acknowledgement.acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
    let received_at = canonical_timestamp(received_at);
    acknowledgement
        .validate()
        .map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let node = queries::node_by_id(
                    transaction,
                    NodeId::from_uuid(acknowledgement.node_id),
                    true,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if node.state == NodeState::Revoked {
                    return Err(RepositoryError::NotFound.into());
                }
                let state = gateway_state(acknowledgement.state);
                if let Some(existing) = fetch_optional::<
                    (
                        Uuid,
                        Option<Uuid>,
                        u64,
                        String,
                        String,
                        Option<String>,
                        DateTime<Utc>,
                    ),
                    _,
                >(
                    transaction,
                    sql_query::<(
                        Uuid,
                        Option<Uuid>,
                        u64,
                        String,
                        String,
                        Option<String>,
                        DateTime<Utc>,
                    )>(
                        "select node_id, command_id, revision, snapshot_digest, state, message, acknowledged_at from node_gateway_acknowledgements where acknowledgement_id = ",
                    )
                    .bind(acknowledgement.acknowledgement_id)
                    .append(" for update"),
                )
                .await?
                {
                    if existing
                        != (
                            acknowledgement.node_id,
                            Some(acknowledgement.command_id),
                            acknowledgement.revision,
                            acknowledgement.snapshot_digest.clone(),
                            state.into(),
                            acknowledgement.message.clone(),
                            acknowledgement.acknowledged_at,
                        )
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway acknowledgement ID was reused with different content".into(),
                        )
                        .into());
                    }
                    return Ok(gateway_receipt(&acknowledgement, true));
                }
                if fetch_optional::<Uuid, _>(
                    transaction,
                    sql_query::<Uuid>(
                        "select acknowledgement_id from node_gateway_acknowledgements where node_id = ",
                    )
                    .bind(acknowledgement.node_id)
                    .append(" and command_id = ")
                    .bind(acknowledgement.command_id)
                    .append(" for update"),
                )
                .await?
                .is_some()
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway command already has an acknowledgement".into(),
                    )
                    .into());
                }
                require_one_row(
                    "Gateway acknowledgement",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "insert into node_gateway_acknowledgements (acknowledgement_id, node_id, command_id, revision, snapshot_digest, state, message, acknowledged_at, received_at) values (",
                        )
                        .bind(acknowledgement.acknowledgement_id)
                        .append(", ")
                        .bind(acknowledgement.node_id)
                        .append(", ")
                        .bind(acknowledgement.command_id)
                        .append(", ")
                        .bind(acknowledgement.revision)
                        .append(", ")
                        .bind(acknowledgement.snapshot_digest.as_str())
                        .append(", ")
                        .bind(state)
                        .append(", ")
                        .bind(acknowledgement.message.clone())
                        .append(", ")
                        .bind(acknowledgement.acknowledged_at)
                        .append(", ")
                        .bind(received_at)
                        .append(")"),
                    )
                    .await?,
                )?;
                Ok(gateway_receipt(&acknowledgement, false))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn record_log_chunks(
    executor: &PostgresExecutor,
    mut batch: NodeLogBatchReceiptDraft,
    received_at: DateTime<Utc>,
) -> Result<NodeLogChunkReceipt, RepositoryError> {
    batch.sent_at = canonical_timestamp(batch.sent_at);
    let received_at = canonical_timestamp(received_at);
    batch.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let node = queries::node_by_id(transaction, batch.node_id, true)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if node.state == NodeState::Revoked {
                    return Err(RepositoryError::NotFound.into());
                }
                if let Some(existing) = fetch_optional::<(Uuid, String, DateTime<Utc>, i32), _>(
                    transaction,
                    sql_query::<(Uuid, String, DateTime<Utc>, i32)>(
                        "select node_id, payload_digest, sent_at, chunk_count from node_log_batches where batch_id = ",
                    )
                    .bind(batch.batch_id)
                    .append(" for update"),
                )
                .await?
                {
                    if existing
                        != (
                            batch.node_id.as_uuid(),
                            batch.payload_digest.clone(),
                            batch.sent_at,
                            i32::try_from(batch.chunks.len()).map_err(|_| {
                                PostgresPersistenceError::Invariant(
                                    "log chunk count overflowed".into(),
                                )
                            })?,
                        )
                        || stored_log_chunks_for_batch(transaction, batch.batch_id).await?
                            != batch
                                .chunks
                                .iter()
                                .map(LogChunkRow::from)
                                .collect::<Vec<_>>()
                    {
                        return Err(RepositoryError::Conflict(
                            "log batch ID was reused with different content".into(),
                        )
                        .into());
                    }
                    return log_receipt(&batch, true);
                }

                require_one_row(
                    "node log batch",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "insert into node_log_batches (batch_id, node_id, payload_digest, sent_at, received_at, chunk_count) values (",
                        )
                        .bind(batch.batch_id)
                        .append(", ")
                        .bind(batch.node_id.as_uuid())
                        .append(", ")
                        .bind(batch.payload_digest.as_str())
                        .append(", ")
                        .bind(batch.sent_at)
                        .append(", ")
                        .bind(received_at)
                        .append(", ")
                        .bind(i32::try_from(batch.chunks.len()).map_err(|_| {
                            PostgresPersistenceError::Invariant(
                                "log chunk count overflowed".into(),
                            )
                        })?)
                        .append(")"),
                    )
                    .await?,
                )?;

                for (ordinal, chunk) in batch.chunks.iter().enumerate() {
                    let existing = fetch_optional::<LogChunkRow, _>(
                        transaction,
                        sql_query::<LogChunkRow>(
                            "select unit_id, generation, cursor_value, sequence, observed_at_ms, stream, checksum, object_key from node_log_chunks where node_id = ",
                        )
                        .bind(batch.node_id.as_uuid())
                        .append(" and unit_id = ")
                        .bind(chunk.unit_id.as_str())
                        .append(" and generation = ")
                        .bind(chunk.generation)
                        .append(" and sequence = ")
                        .bind(chunk.sequence)
                        .append(" for update"),
                    )
                    .await?;
                    if let Some(existing) = existing {
                        if existing != LogChunkRow::from(chunk) {
                            return Err(RepositoryError::Conflict(
                                "log sequence was reused with different content".into(),
                            )
                            .into());
                        }
                    } else {
                        if fetch_optional::<u64, _>(
                            transaction,
                            sql_query::<u64>(
                                "select sequence from node_log_chunks where node_id = ",
                            )
                            .bind(batch.node_id.as_uuid())
                            .append(" and unit_id = ")
                            .bind(chunk.unit_id.as_str())
                            .append(" and generation = ")
                            .bind(chunk.generation)
                            .append(" and cursor_value = ")
                            .bind(chunk.cursor.as_str())
                            .append(" for update"),
                        )
                        .await?
                        .is_some()
                        {
                            return Err(RepositoryError::Conflict(
                                "log cursor was reused for another sequence".into(),
                            )
                            .into());
                        }
                        require_one_row(
                            "node log chunk",
                            execute(
                                transaction,
                                sql_query::<()>(
                                    "insert into node_log_chunks (node_id, unit_id, generation, cursor_value, sequence, observed_at_ms, stream, checksum, received_at, object_key) values (",
                                )
                                .bind(batch.node_id.as_uuid())
                                .append(", ")
                                .bind(chunk.unit_id.as_str())
                                .append(", ")
                                .bind(chunk.generation)
                                .append(", ")
                                .bind(chunk.cursor.as_str())
                                .append(", ")
                                .bind(chunk.sequence)
                                .append(", ")
                                .bind(chunk.observed_at_ms)
                                .append(", ")
                                .bind(chunk.stream.as_str())
                                .append(", ")
                                .bind(chunk.checksum.as_str())
                                .append(", ")
                                .bind(received_at)
                                .append(", ")
                                .bind(chunk.object_key.as_str())
                                .append(")"),
                            )
                            .await?,
                        )?;
                    }
                    require_one_row(
                        "node log batch chunk",
                        execute(
                            transaction,
                            sql_query::<()>(
                                "insert into node_log_batch_chunks (batch_id, ordinal, node_id, unit_id, generation, sequence) values (",
                            )
                            .bind(batch.batch_id)
                            .append(", ")
                            .bind(i32::try_from(ordinal).map_err(|_| {
                                PostgresPersistenceError::Invariant(
                                    "log chunk ordinal overflowed".into(),
                                )
                            })?)
                            .append(", ")
                            .bind(batch.node_id.as_uuid())
                            .append(", ")
                            .bind(chunk.unit_id.as_str())
                            .append(", ")
                            .bind(chunk.generation)
                            .append(", ")
                            .bind(chunk.sequence)
                            .append(")"),
                        )
                        .await?,
                    )?;
                }
                log_receipt(&batch, false)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn replay_log_batch(
    executor: &PostgresExecutor,
    mut batch: NodeLogBatchReplay,
) -> Result<Option<NodeLogChunkReceipt>, RepositoryError> {
    batch.sent_at = canonical_timestamp(batch.sent_at);
    batch.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let existing = fetch_optional::<(Uuid, String, DateTime<Utc>, i32), _>(
                    transaction,
                    sql_query::<(Uuid, String, DateTime<Utc>, i32)>(
                        "select node_id, payload_digest, sent_at, chunk_count from node_log_batches where batch_id = ",
                    )
                    .bind(batch.batch_id),
                )
                .await?;
                let Some(existing) = existing else {
                    return Ok(None);
                };
                if existing
                    != (
                        batch.node_id.as_uuid(),
                        batch.payload_digest.clone(),
                        batch.sent_at,
                        i32::from(batch.chunk_count),
                    )
                {
                    return Err(RepositoryError::Conflict(
                        "log batch ID was reused with different content".into(),
                    )
                    .into());
                }
                Ok(Some(batch.receipt()))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn list_log_chunks(
    executor: &PostgresExecutor,
    query: NodeLogChunkQuery,
) -> Result<Vec<NodeLogChunkMetadata>, RepositoryError> {
    query.validate().map_err(RepositoryError::Conflict)?;
    let limit = i64::try_from(query.limit)
        .map_err(|_| RepositoryError::Conflict("log chunk query limit is invalid".into()))?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut statement = sql_query::<LogChunkMetadataRow>(
                    "select unit_id, generation, cursor_value, sequence, observed_at_ms, stream, checksum, object_key, retained_at from node_log_chunks where node_id = ",
                )
                .bind(query.node_id.as_uuid())
                .append(" and unit_id = ")
                .bind(query.unit_id.as_str())
                .append(" and generation = ")
                .bind(query.generation);
                if let Some(after_sequence) = query.after_sequence {
                    statement = statement
                        .append(" and sequence > ")
                        .bind(after_sequence);
                }
                if let Some(stream) = query.stream {
                    statement = statement.append(" and stream = ").bind(match stream {
                        RuntimeLogStream::Stdout => "stdout",
                        RuntimeLogStream::Stderr => "stderr",
                    });
                }
                let rows = fetch_all(
                    transaction,
                    statement.append(" order by sequence limit ").bind(limit),
                )
                .await?;
                rows.into_iter()
                    .map(|row| row.metadata(query.node_id))
                    .collect()
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn list_log_chunks_for_retention(
    executor: &PostgresExecutor,
    received_before: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<NodeLogRetentionTarget>, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "log retention query limit must be between 1 and 10000".into(),
        ));
    }
    let received_before = canonical_timestamp(received_before);
    let limit = i64::try_from(limit)
        .map_err(|_| RepositoryError::Conflict("log retention limit is invalid".into()))?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let rows = fetch_all(
                    transaction,
                    sql_query::<LogRetentionRow>(
                        "select node_id, unit_id, generation, sequence, object_key, received_at from node_log_chunks where retained_at is null and received_at < ",
                    )
                    .bind(received_before)
                    .append(
                        " order by received_at, node_id, unit_id, generation, sequence limit ",
                    )
                    .bind(limit),
                )
                .await?;
                rows.into_iter().map(LogRetentionRow::target).collect()
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn mark_log_chunk_retained(
    executor: &PostgresExecutor,
    target: &NodeLogRetentionTarget,
    retained_at: DateTime<Utc>,
) -> Result<bool, RepositoryError> {
    target.validate().map_err(RepositoryError::Conflict)?;
    let target = target.clone();
    let retained_at = canonical_timestamp(retained_at);
    if retained_at < canonical_timestamp(target.received_at) {
        return Err(RepositoryError::Conflict(
            "log retention timestamp precedes receipt".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let rows = execute(
                    transaction,
                    sql_query::<()>("update node_log_chunks set retained_at = ")
                        .bind(retained_at)
                        .append(" where node_id = ")
                        .bind(target.node_id.as_uuid())
                        .append(" and unit_id = ")
                        .bind(target.unit_id.as_str())
                        .append(" and generation = ")
                        .bind(target.generation)
                        .append(" and sequence = ")
                        .bind(target.sequence)
                        .append(" and object_key = ")
                        .bind(target.object_key.as_str())
                        .append(" and received_at = ")
                        .bind(canonical_timestamp(target.received_at))
                        .append(" and retained_at is null"),
                )
                .await?;
                match rows {
                    0 => Ok(false),
                    1 => Ok(true),
                    actual => Err(PostgresPersistenceError::Invariant(format!(
                        "log retention updated {actual} rows"
                    ))),
                }
            })
        })
        .await
        .map_err(transaction_error)
}

async fn stored_log_chunks_for_batch(
    transaction: &a3s_orm::PostgresTransaction,
    batch_id: Uuid,
) -> Result<Vec<LogChunkRow>, PostgresPersistenceError> {
    fetch_all(
        transaction,
        sql_query::<LogChunkRow>(
            "select chunks.unit_id, chunks.generation, chunks.cursor_value, chunks.sequence, chunks.observed_at_ms, chunks.stream, chunks.checksum, chunks.object_key from node_log_batch_chunks as members join node_log_chunks as chunks on chunks.node_id = members.node_id and chunks.unit_id = members.unit_id and chunks.generation = members.generation and chunks.sequence = members.sequence where members.batch_id = ",
        )
        .bind(batch_id)
        .append(" order by members.ordinal"),
    )
    .await
}

const fn gateway_state(state: GatewayAckState) -> &'static str {
    match state {
        GatewayAckState::Applied => "applied",
        GatewayAckState::Rejected => "rejected",
    }
}

fn gateway_receipt(acknowledgement: &NodeGatewayAck, replayed: bool) -> NodeGatewayAckReceipt {
    NodeGatewayAckReceipt {
        schema: NodeGatewayAckReceipt::SCHEMA.into(),
        acknowledgement_id: acknowledgement.acknowledgement_id,
        command_id: acknowledgement.command_id,
        node_id: acknowledgement.node_id,
        replayed,
    }
}

fn log_receipt(
    batch: &NodeLogBatchReceiptDraft,
    replayed: bool,
) -> Result<NodeLogChunkReceipt, PostgresPersistenceError> {
    let receipt = NodeLogChunkReceipt {
        schema: NodeLogChunkReceipt::SCHEMA.into(),
        batch_id: batch.batch_id,
        node_id: batch.node_id.as_uuid(),
        accepted_chunks: u16::try_from(batch.chunks.len()).map_err(|_| {
            PostgresPersistenceError::Invariant("log chunk count overflowed".into())
        })?,
        replayed,
    };
    receipt
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    Ok(receipt)
}
