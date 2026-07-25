use super::super::schema::{
    NodeResourceInventories, NodeResourceInventoryHeads, NodeResourceInventorySlots, Nodes,
};
use crate::infrastructure::{
    execute, fetch_optional, require_one_row, transaction_error, PostgresPersistenceError,
};
use crate::modules::fleet::domain::repositories::NodeResourceInventoryRecord;
use crate::modules::shared_kernel::domain::{canonical_timestamp, NodeId, RepositoryError};
use a3s_cloud_contracts::{
    NodeInventoryReference, NodeResourceInventory, NodeResourceInventoryReceipt, NodeResourceSlot,
};
use a3s_orm::{
    insert_into, select_from, update_table, Database, InsertRow, PostgresDialect, PostgresExecutor,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

type StoredInventoryRow = (Uuid, String, Uuid, DateTime<Utc>, DateTime<Utc>, Value);
type InventoryHeadRow = (Uuid, u64, String, Uuid, DateTime<Utc>, DateTime<Utc>);
type CurrentInventoryRow = (
    Uuid,
    Uuid,
    u64,
    String,
    Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
    Value,
);

pub(in super::super) async fn record(
    executor: &PostgresExecutor,
    mut inventory: NodeResourceInventory,
    received_at: DateTime<Utc>,
) -> Result<NodeResourceInventoryReceipt, RepositoryError> {
    inventory.validate().map_err(RepositoryError::Conflict)?;
    inventory.observed_at = canonical_timestamp(inventory.observed_at);
    let received_at = canonical_timestamp(received_at);
    inventory.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                transaction
                    .advisory_xact_lock(
                        "a3s.cloud.node-resource-inventory",
                        &inventory.node_id.to_string(),
                    )
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
                            .filter(Nodes::id().eq(inventory.node_id))
                            .for_update(),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if state == "revoked" {
                    return Err(RepositoryError::NotFound.into());
                }
                if agent_instance_id != inventory.agent_instance_id {
                    return Err(RepositoryError::Conflict(
                        "resource inventory agent identity does not match the enrolled node".into(),
                    )
                    .into());
                }

                if let Some(row) = fetch_optional::<StoredInventoryRow, _>(
                    transaction,
                    select_from::<NodeResourceInventories>()
                        .select((
                            NodeResourceInventories::organization_id(),
                            NodeResourceInventories::inventory_digest(),
                            NodeResourceInventories::agent_instance_id(),
                            NodeResourceInventories::observed_at(),
                            NodeResourceInventories::received_at(),
                            NodeResourceInventories::snapshot(),
                        ))
                        .filter(NodeResourceInventories::node_id().eq(inventory.node_id))
                        .filter(NodeResourceInventories::generation().eq(inventory.generation))
                        .for_update(),
                )
                .await?
                {
                    let existing = restore_inventory(inventory.node_id, inventory.generation, row)?;
                    if existing.inventory != inventory {
                        return Err(RepositoryError::Conflict(
                            "resource inventory generation was reused with different content"
                                .into(),
                        )
                        .into());
                    }
                    return inventory_receipt(&inventory, true);
                }

                let head = fetch_optional::<InventoryHeadRow, _>(
                    transaction,
                    select_from::<NodeResourceInventoryHeads>()
                        .select((
                            NodeResourceInventoryHeads::organization_id(),
                            NodeResourceInventoryHeads::generation(),
                            NodeResourceInventoryHeads::inventory_digest(),
                            NodeResourceInventoryHeads::agent_instance_id(),
                            NodeResourceInventoryHeads::observed_at(),
                            NodeResourceInventoryHeads::received_at(),
                        ))
                        .filter(NodeResourceInventoryHeads::node_id().eq(inventory.node_id))
                        .for_update(),
                )
                .await?;
                if let Some((
                    head_organization_id,
                    generation,
                    digest,
                    head_agent_instance_id,
                    observed_at,
                    _,
                )) = &head
                {
                    if *head_organization_id != organization_id
                        || *head_agent_instance_id != inventory.agent_instance_id
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "stored resource inventory head has inconsistent ownership".into(),
                        ));
                    }
                    let next_generation = generation.checked_add(1).ok_or_else(|| {
                        RepositoryError::Conflict(
                            "resource inventory generation is exhausted".into(),
                        )
                    })?;
                    if inventory.generation != next_generation {
                        return Err(RepositoryError::Conflict(
                            "resource inventory generation did not advance exactly once".into(),
                        )
                        .into());
                    }
                    if inventory.observed_at <= *observed_at {
                        return Err(RepositoryError::Conflict(
                            "resource inventory observation time did not advance".into(),
                        )
                        .into());
                    }
                    if inventory.digest == *digest {
                        return Err(RepositoryError::Conflict(
                            "resource inventory generation advanced without a content change"
                                .into(),
                        )
                        .into());
                    }
                } else if inventory.generation != 1 {
                    return Err(RepositoryError::Conflict(
                        "first resource inventory generation must be one".into(),
                    )
                    .into());
                }

                let snapshot = serde_json::to_value(&inventory)?;
                require_one_row(
                    "node resource inventory",
                    execute(
                        transaction,
                        insert_into::<NodeResourceInventories>()
                            .value(NodeResourceInventories::organization_id(), organization_id)
                            .value(NodeResourceInventories::node_id(), inventory.node_id)
                            .value(NodeResourceInventories::generation(), inventory.generation)
                            .value(
                                NodeResourceInventories::inventory_digest(),
                                inventory.digest.as_str(),
                            )
                            .value(
                                NodeResourceInventories::agent_instance_id(),
                                inventory.agent_instance_id,
                            )
                            .value(
                                NodeResourceInventories::observed_at(),
                                inventory.observed_at,
                            )
                            .value(NodeResourceInventories::received_at(), received_at)
                            .value(NodeResourceInventories::snapshot(), snapshot),
                    )
                    .await?,
                )?;
                insert_slots(
                    transaction,
                    organization_id,
                    inventory.node_id,
                    inventory.generation,
                    &inventory.slots,
                )
                .await?;

                match head {
                    Some((_, generation, digest, _, _, _)) => {
                        require_one_row(
                            "node resource inventory head",
                            execute(
                                transaction,
                                update_table::<NodeResourceInventoryHeads>()
                                    .set(
                                        NodeResourceInventoryHeads::generation(),
                                        inventory.generation,
                                    )
                                    .set(
                                        NodeResourceInventoryHeads::inventory_digest(),
                                        inventory.digest.as_str(),
                                    )
                                    .set(
                                        NodeResourceInventoryHeads::agent_instance_id(),
                                        inventory.agent_instance_id,
                                    )
                                    .set(
                                        NodeResourceInventoryHeads::observed_at(),
                                        inventory.observed_at,
                                    )
                                    .set(NodeResourceInventoryHeads::received_at(), received_at)
                                    .filter(
                                        NodeResourceInventoryHeads::node_id().eq(inventory.node_id),
                                    )
                                    .filter(NodeResourceInventoryHeads::generation().eq(generation))
                                    .filter(
                                        NodeResourceInventoryHeads::inventory_digest()
                                            .eq(digest.as_str()),
                                    ),
                            )
                            .await?,
                        )?;
                    }
                    None => {
                        require_one_row(
                            "node resource inventory head",
                            execute(
                                transaction,
                                insert_into::<NodeResourceInventoryHeads>()
                                    .value(
                                        NodeResourceInventoryHeads::organization_id(),
                                        organization_id,
                                    )
                                    .value(NodeResourceInventoryHeads::node_id(), inventory.node_id)
                                    .value(
                                        NodeResourceInventoryHeads::generation(),
                                        inventory.generation,
                                    )
                                    .value(
                                        NodeResourceInventoryHeads::inventory_digest(),
                                        inventory.digest.as_str(),
                                    )
                                    .value(
                                        NodeResourceInventoryHeads::agent_instance_id(),
                                        inventory.agent_instance_id,
                                    )
                                    .value(
                                        NodeResourceInventoryHeads::observed_at(),
                                        inventory.observed_at,
                                    )
                                    .value(NodeResourceInventoryHeads::received_at(), received_at),
                            )
                            .await?,
                        )?;
                    }
                }
                inventory_receipt(&inventory, false)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn current(
    executor: &PostgresExecutor,
    node_id: NodeId,
) -> Result<Option<NodeResourceInventoryRecord>, RepositoryError> {
    let row = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<NodeResourceInventoryHeads>()
                .select((
                    NodeResourceInventoryHeads::organization_id(),
                    NodeResourceInventoryHeads::node_id(),
                    NodeResourceInventoryHeads::generation(),
                    NodeResourceInventoryHeads::inventory_digest(),
                    NodeResourceInventoryHeads::agent_instance_id(),
                    NodeResourceInventoryHeads::observed_at(),
                    NodeResourceInventoryHeads::received_at(),
                    NodeResourceInventories::snapshot(),
                ))
                .inner_join::<NodeResourceInventories>(
                    NodeResourceInventoryHeads::node_id()
                        .eq_column(NodeResourceInventories::node_id())
                        .and(
                            NodeResourceInventoryHeads::generation()
                                .eq_column(NodeResourceInventories::generation()),
                        )
                        .and(
                            NodeResourceInventoryHeads::inventory_digest()
                                .eq_column(NodeResourceInventories::inventory_digest()),
                        ),
                )
                .filter(NodeResourceInventoryHeads::node_id().eq(node_id.as_uuid())),
        )
        .await
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    row.map(restore_current_inventory).transpose()
}

pub(super) async fn require_current_reference(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: Uuid,
    agent_instance_id: Uuid,
    reference: &NodeInventoryReference,
) -> Result<(), PostgresPersistenceError> {
    reference.validate().map_err(RepositoryError::Conflict)?;
    let current = fetch_optional::<(u64, String, Uuid), _>(
        transaction,
        select_from::<NodeResourceInventoryHeads>()
            .select((
                NodeResourceInventoryHeads::generation(),
                NodeResourceInventoryHeads::inventory_digest(),
                NodeResourceInventoryHeads::agent_instance_id(),
            ))
            .filter(NodeResourceInventoryHeads::node_id().eq(node_id)),
    )
    .await?
    .ok_or_else(|| {
        RepositoryError::Conflict("node heartbeat references an unknown resource inventory".into())
    })?;
    if current
        != (
            reference.generation,
            reference.digest.clone(),
            agent_instance_id,
        )
    {
        return Err(RepositoryError::Conflict(
            "node heartbeat does not reference the current resource inventory".into(),
        )
        .into());
    }
    Ok(())
}

async fn insert_slots(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: Uuid,
    node_id: Uuid,
    generation: u64,
    slots: &[NodeResourceSlot],
) -> Result<(), PostgresPersistenceError> {
    let mut rows = Vec::with_capacity(slots.len());
    for (ordinal, slot) in slots.iter().enumerate() {
        rows.push(
            InsertRow::new()
                .value(
                    NodeResourceInventorySlots::organization_id(),
                    organization_id,
                )
                .value(NodeResourceInventorySlots::node_id(), node_id)
                .value(
                    NodeResourceInventorySlots::inventory_generation(),
                    generation,
                )
                .value(
                    NodeResourceInventorySlots::ordinal(),
                    u32::try_from(ordinal).map_err(|_| {
                        PostgresPersistenceError::Invariant(
                            "resource inventory slot ordinal overflowed".into(),
                        )
                    })?,
                )
                .value(
                    NodeResourceInventorySlots::resource_kind(),
                    slot.kind.as_str(),
                )
                .value(
                    NodeResourceInventorySlots::stable_resource_id(),
                    slot.stable_resource_id.as_str(),
                )
                .value(
                    NodeResourceInventorySlots::allocation(),
                    serde_json::to_value(&slot.allocation)?,
                ),
        );
    }
    let inserted = execute(
        transaction,
        insert_into::<NodeResourceInventorySlots>().rows(rows),
    )
    .await?;
    if usize::try_from(inserted).ok() != Some(slots.len()) {
        return Err(PostgresPersistenceError::Invariant(format!(
            "writing resource inventory slots affected {inserted} rows"
        )));
    }
    Ok(())
}

fn restore_inventory(
    node_id: Uuid,
    generation: u64,
    (organization_id, digest, agent_instance_id, observed_at, received_at, snapshot): StoredInventoryRow,
) -> Result<NodeResourceInventoryRecord, PostgresPersistenceError> {
    let inventory: NodeResourceInventory = serde_json::from_value(snapshot)?;
    inventory
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if organization_id.is_nil()
        || inventory.node_id != node_id
        || inventory.generation != generation
        || inventory.digest != digest
        || inventory.agent_instance_id != agent_instance_id
        || inventory.observed_at != observed_at
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored node resource inventory metadata is inconsistent".into(),
        ));
    }
    let record = NodeResourceInventoryRecord {
        inventory,
        received_at,
    };
    record
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    Ok(record)
}

fn restore_current_inventory(
    (
        organization_id,
        node_id,
        generation,
        digest,
        agent_instance_id,
        observed_at,
        received_at,
        snapshot,
    ): CurrentInventoryRow,
) -> Result<NodeResourceInventoryRecord, RepositoryError> {
    restore_inventory(
        node_id,
        generation,
        (
            organization_id,
            digest,
            agent_instance_id,
            observed_at,
            received_at,
            snapshot,
        ),
    )
    .map_err(|error| RepositoryError::Storage(error.to_string()))
}

fn inventory_receipt(
    inventory: &NodeResourceInventory,
    replayed: bool,
) -> Result<NodeResourceInventoryReceipt, PostgresPersistenceError> {
    let receipt = NodeResourceInventoryReceipt {
        schema: NodeResourceInventoryReceipt::SCHEMA.into(),
        node_id: inventory.node_id,
        generation: inventory.generation,
        digest: inventory.digest.clone(),
        replayed,
    };
    receipt
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    Ok(receipt)
}
