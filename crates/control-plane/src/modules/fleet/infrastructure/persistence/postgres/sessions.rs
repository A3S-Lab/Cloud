use super::schema::{NodeProtocolSessionHeads, Nodes};
use crate::infrastructure::{execute, fetch_optional, require_one_row, transaction_error};
use crate::modules::fleet::domain::value_objects::{
    NodeProtocolNegotiation, NodeProtocolNegotiationOutcome, NodeProtocolSessionRecord,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{NodeSessionHello, NodeSessionSelection};
use a3s_orm::{insert_into, select_from, update_table, PostgresExecutor};
use serde_json::Value;
use uuid::Uuid;

type StoredSessionRow = (String, Value, Value);

pub(super) async fn negotiate(
    executor: &PostgresExecutor,
    negotiation: NodeProtocolNegotiation,
) -> Result<NodeProtocolNegotiationOutcome, RepositoryError> {
    let node_id = negotiation.hello().node_id;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                transaction
                    .advisory_xact_lock("a3s.cloud.node-protocol-session", &node_id.to_string())
                    .await?;
                let (organization_id, state, agent_instance_id) =
                    fetch_optional::<(Uuid, String, Uuid), _>(
                        transaction,
                        select_from::<Nodes>()
                            .select((
                                Nodes::organization_id(),
                                Nodes::state(),
                                Nodes::agent_instance_id(),
                            ))
                            .filter(Nodes::id().eq(node_id))
                            .for_update(),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if state == "revoked" {
                    return Err(RepositoryError::NotFound.into());
                }
                if agent_instance_id != negotiation.hello().agent_instance_id {
                    return Err(RepositoryError::Forbidden(
                        "node session Agent instance does not match the enrolled node".into(),
                    )
                    .into());
                }

                let stored = fetch_optional::<StoredSessionRow, _>(
                    transaction,
                    select_from::<NodeProtocolSessionHeads>()
                        .select((
                            NodeProtocolSessionHeads::contracts_digest(),
                            NodeProtocolSessionHeads::hello(),
                            NodeProtocolSessionHeads::selection(),
                        ))
                        .filter(NodeProtocolSessionHeads::node_id().eq(node_id))
                        .for_update(),
                )
                .await?;
                let current = stored.map(restore).transpose()?;
                let outcome = negotiation
                    .apply(current.as_ref())
                    .map_err(|error| RepositoryError::Conflict(error.to_string()))?;
                if outcome.replayed() {
                    return Ok(outcome);
                }

                let record = outcome.record();
                let selection = record.selection();
                let contracts_digest = record
                    .reference()
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?
                    .contracts_digest;
                let hello_document = serde_json::to_value(record.hello())?;
                let selection_document = serde_json::to_value(selection)?;
                match current {
                    Some(current) => {
                        require_one_row(
                            "node protocol session head",
                            execute(
                                transaction,
                                update_table::<NodeProtocolSessionHeads>()
                                    .set(
                                        NodeProtocolSessionHeads::agent_instance_id(),
                                        selection.agent_instance_id,
                                    )
                                    .set(
                                        NodeProtocolSessionHeads::session_epoch(),
                                        selection.session_epoch,
                                    )
                                    .set(
                                        NodeProtocolSessionHeads::hello_sequence(),
                                        selection.hello_sequence,
                                    )
                                    .set(
                                        NodeProtocolSessionHeads::session_id(),
                                        selection.session_id,
                                    )
                                    .set(
                                        NodeProtocolSessionHeads::generation(),
                                        selection.generation,
                                    )
                                    .set(
                                        NodeProtocolSessionHeads::contracts_digest(),
                                        contracts_digest.as_str(),
                                    )
                                    .set(
                                        NodeProtocolSessionHeads::selected_at(),
                                        selection.selected_at,
                                    )
                                    .set(
                                        NodeProtocolSessionHeads::expires_at(),
                                        selection.expires_at,
                                    )
                                    .set(NodeProtocolSessionHeads::hello(), hello_document)
                                    .set(NodeProtocolSessionHeads::selection(), selection_document)
                                    .filter(NodeProtocolSessionHeads::node_id().eq(node_id))
                                    .filter(
                                        NodeProtocolSessionHeads::generation()
                                            .eq(current.selection().generation),
                                    )
                                    .filter(
                                        NodeProtocolSessionHeads::session_id()
                                            .eq(current.selection().session_id),
                                    ),
                            )
                            .await?,
                        )?;
                    }
                    None => {
                        require_one_row(
                            "node protocol session head",
                            execute(
                                transaction,
                                insert_into::<NodeProtocolSessionHeads>()
                                    .value(
                                        NodeProtocolSessionHeads::organization_id(),
                                        organization_id,
                                    )
                                    .value(NodeProtocolSessionHeads::node_id(), node_id)
                                    .value(
                                        NodeProtocolSessionHeads::agent_instance_id(),
                                        selection.agent_instance_id,
                                    )
                                    .value(
                                        NodeProtocolSessionHeads::session_epoch(),
                                        selection.session_epoch,
                                    )
                                    .value(
                                        NodeProtocolSessionHeads::hello_sequence(),
                                        selection.hello_sequence,
                                    )
                                    .value(
                                        NodeProtocolSessionHeads::session_id(),
                                        selection.session_id,
                                    )
                                    .value(
                                        NodeProtocolSessionHeads::generation(),
                                        selection.generation,
                                    )
                                    .value(
                                        NodeProtocolSessionHeads::contracts_digest(),
                                        contracts_digest.as_str(),
                                    )
                                    .value(
                                        NodeProtocolSessionHeads::selected_at(),
                                        selection.selected_at,
                                    )
                                    .value(
                                        NodeProtocolSessionHeads::expires_at(),
                                        selection.expires_at,
                                    )
                                    .value(NodeProtocolSessionHeads::hello(), hello_document)
                                    .value(
                                        NodeProtocolSessionHeads::selection(),
                                        selection_document,
                                    ),
                            )
                            .await?,
                        )?;
                    }
                }
                Ok(outcome)
            })
        })
        .await
        .map_err(transaction_error)
}

fn restore(row: StoredSessionRow) -> Result<NodeProtocolSessionRecord, RepositoryError> {
    let (stored_digest, hello_document, selection_document) = row;
    let hello: NodeSessionHello = serde_json::from_value(hello_document).map_err(|error| {
        RepositoryError::Storage(format!("stored node session hello is invalid: {error}"))
    })?;
    let selection: NodeSessionSelection =
        serde_json::from_value(selection_document).map_err(|error| {
            RepositoryError::Storage(format!("stored node session selection is invalid: {error}"))
        })?;
    let record = NodeProtocolSessionRecord::restore(hello, selection).map_err(|error| {
        RepositoryError::Storage(format!("stored node protocol session is invalid: {error}"))
    })?;
    let actual_digest = record
        .reference()
        .map_err(|error| RepositoryError::Storage(error.to_string()))?
        .contracts_digest;
    if actual_digest != stored_digest {
        return Err(RepositoryError::Storage(
            "stored node protocol session contract digest does not match its selection".into(),
        ));
    }
    Ok(record)
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
