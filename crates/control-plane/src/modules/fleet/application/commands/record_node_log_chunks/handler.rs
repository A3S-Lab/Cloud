use super::RecordNodeLogChunks;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, NodeLogBatchReceiptDraft, NodeLogBatchReplay, NodeLogChunkReceiptDraft,
    NodeLogGapReceiptDraft,
};
use crate::modules::fleet::domain::services::{ILogChunkStore, LogChunkStoreError, StoredLogChunk};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::{CommandHandler, CqrsContext};
use a3s_runtime::contract::RuntimeLogStream;
use std::sync::Arc;

pub struct RecordNodeLogChunksHandler {
    nodes: Arc<dyn INodeControlRepository>,
    objects: Arc<dyn ILogChunkStore>,
}

impl RecordNodeLogChunksHandler {
    pub fn new(nodes: Arc<dyn INodeControlRepository>, objects: Arc<dyn ILogChunkStore>) -> Self {
        Self { nodes, objects }
    }
}

impl CommandHandler<RecordNodeLogChunks> for RecordNodeLogChunksHandler {
    fn execute(
        &self,
        command: RecordNodeLogChunks,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<a3s_cloud_contracts::NodeLogChunkReceipt>>,
    > {
        let nodes = Arc::clone(&self.nodes);
        let objects = Arc::clone(&self.objects);
        Box::pin(async move {
            if command.batch.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the log batch".into(),
                )));
            }
            if let Err(error) = command.batch.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let payload_digest = match command.batch.digest() {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let chunk_count = match u16::try_from(command.batch.chunks.len()) {
                Ok(chunk_count) => chunk_count,
                Err(_) => {
                    return Ok(Err(ApplicationError::Invalid(
                        "log batch chunk count exceeds the protocol bound".into(),
                    )))
                }
            };
            let gap_count = match u16::try_from(command.batch.gaps.len()) {
                Ok(gap_count) => gap_count,
                Err(_) => {
                    return Ok(Err(ApplicationError::Invalid(
                        "log batch gap count exceeds the protocol bound".into(),
                    )))
                }
            };
            let replay = NodeLogBatchReplay {
                batch_id: command.batch.batch_id,
                node_id: command.authenticated_node_id,
                payload_digest: payload_digest.clone(),
                sent_at: command.batch.sent_at,
                chunk_count,
                gap_count,
            };
            match nodes.replay_log_batch(replay).await {
                Ok(Some(receipt)) => return Ok(Ok(receipt)),
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let mut stored = Vec::with_capacity(command.batch.chunks.len());
            let mut receipts = Vec::with_capacity(command.batch.chunks.len());
            for (ordinal, report) in command.batch.chunks.iter().enumerate() {
                let ordinal = match u16::try_from(ordinal) {
                    Ok(value) => value,
                    Err(_) => {
                        return Ok(Err(ApplicationError::Invalid(
                            "log chunk ordinal exceeds the protocol bound".into(),
                        )))
                    }
                };
                let object = match objects
                    .put(
                        command.batch.batch_id,
                        command.batch.node_id,
                        ordinal,
                        report,
                    )
                    .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        return Ok(Err(cleanup_or_store_error(&objects, &stored, error).await));
                    }
                };
                receipts.push(NodeLogChunkReceiptDraft {
                    unit_id: report.unit_id.clone(),
                    generation: report.generation,
                    cursor: report.chunk.cursor.clone(),
                    sequence: report.chunk.sequence,
                    observed_at_ms: report.chunk.observed_at_ms,
                    stream: match report.chunk.stream {
                        RuntimeLogStream::Stdout => "stdout",
                        RuntimeLogStream::Stderr => "stderr",
                    }
                    .into(),
                    checksum: report.checksum.clone(),
                    object_key: object.object_key.clone(),
                });
                stored.push(object);
            }
            let gaps = command
                .batch
                .gaps
                .iter()
                .map(|gap| NodeLogGapReceiptDraft {
                    unit_id: gap.unit_id.clone(),
                    generation: gap.generation,
                    cursor: gap.cursor.clone(),
                    sequence: gap.sequence,
                    observed_at_ms: gap.observed_at_ms,
                    reason: gap.reason,
                })
                .collect();
            let draft = NodeLogBatchReceiptDraft {
                batch_id: command.batch.batch_id,
                node_id: NodeId::from_uuid(command.batch.node_id),
                payload_digest,
                sent_at: command.batch.sent_at,
                chunks: receipts,
                gaps,
            };
            Ok(
                match nodes.record_log_chunks(draft, command.received_at).await {
                    Ok(receipt) => Ok(receipt),
                    Err(error) => match cleanup_created(&objects, &stored).await {
                        Ok(()) => Err(error.into()),
                        Err(cleanup) => Err(ApplicationError::Internal(format!(
                        "could not persist log receipt ({error}); cleanup also failed ({cleanup})"
                    ))),
                    },
                },
            )
        })
    }
}

async fn cleanup_or_store_error(
    objects: &Arc<dyn ILogChunkStore>,
    stored: &[StoredLogChunk],
    error: LogChunkStoreError,
) -> ApplicationError {
    match cleanup_created(objects, stored).await {
        Ok(()) => store_error(error),
        Err(cleanup) => ApplicationError::Internal(format!(
            "log object write failed ({error}); cleanup also failed ({cleanup})"
        )),
    }
}

async fn cleanup_created(
    objects: &Arc<dyn ILogChunkStore>,
    stored: &[StoredLogChunk],
) -> Result<(), LogChunkStoreError> {
    for object in stored.iter().filter(|object| object.created) {
        objects.remove(&object.object_key).await?;
    }
    Ok(())
}

fn store_error(error: LogChunkStoreError) -> ApplicationError {
    match error {
        LogChunkStoreError::Invalid(message) => ApplicationError::Invalid(message),
        LogChunkStoreError::Conflict(message) => ApplicationError::Conflict(message),
        LogChunkStoreError::Unavailable(message) => ApplicationError::Internal(message),
    }
}
