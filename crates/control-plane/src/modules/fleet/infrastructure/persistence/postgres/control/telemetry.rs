use super::*;

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

pub(in super::super) async fn record_gateway_acknowledgement(
    executor: &PostgresExecutor,
    acknowledgement: NodeGatewayAck,
    received_at: DateTime<Utc>,
) -> Result<NodeGatewayAckReceipt, RepositoryError> {
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
                    (Uuid, u64, String, String, Option<String>, DateTime<Utc>),
                    _,
                >(
                    transaction,
                    sql_query::<(Uuid, u64, String, String, Option<String>, DateTime<Utc>)>(
                        "select node_id, revision, snapshot_digest, state, message, acknowledged_at from node_gateway_acknowledgements where acknowledgement_id = ",
                    )
                    .bind(acknowledgement.acknowledgement_id)
                    .append(" for update"),
                )
                .await?
                {
                    if existing
                        != (
                            acknowledgement.node_id,
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
                    .append(" and revision = ")
                    .bind(acknowledgement.revision)
                    .append(" and snapshot_digest = ")
                    .bind(acknowledgement.snapshot_digest.as_str())
                    .append(" for update"),
                )
                .await?
                .is_some()
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway revision already has an acknowledgement".into(),
                    )
                    .into());
                }
                require_one_row(
                    "Gateway acknowledgement",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "insert into node_gateway_acknowledgements (acknowledgement_id, node_id, revision, snapshot_digest, state, message, acknowledged_at, received_at) values (",
                        )
                        .bind(acknowledgement.acknowledgement_id)
                        .append(", ")
                        .bind(acknowledgement.node_id)
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
    batch: NodeLogBatchReceiptDraft,
    received_at: DateTime<Utc>,
) -> Result<NodeLogChunkReceipt, RepositoryError> {
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
