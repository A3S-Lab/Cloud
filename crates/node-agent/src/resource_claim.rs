use crate::{CommandJournalError, ResourceInventoryError};
use a3s_cloud_contracts::{
    NodeResourceClaimBinding, NodeResourceClaimPrepare, NodeResourceInventory,
};
use std::collections::BTreeMap;

pub(crate) fn validate_prepare(
    request: &NodeResourceClaimPrepare,
    inventory: &NodeResourceInventory,
    active: &[NodeResourceClaimBinding],
) -> Result<(), ResourceClaimExecutionError> {
    request
        .validate()
        .map_err(ResourceClaimExecutionError::Invalid)?;
    request
        .binding
        .validate_inventory(inventory)
        .map_err(ResourceClaimExecutionError::Conflict)?;
    if active.iter().any(|candidate| {
        candidate.claim_id == request.binding.claim_id
            || candidate.runtime_unit_id == request.binding.runtime_unit_id
                && candidate.runtime_generation == request.binding.runtime_generation
    }) {
        return Err(ResourceClaimExecutionError::Conflict(
            "resource claim or Runtime generation is already prepared".into(),
        ));
    }

    let capacities = inventory
        .slots
        .iter()
        .map(|slot| {
            (
                (slot.kind, slot.stable_resource_id.as_str()),
                &slot.allocation,
            )
        })
        .collect::<BTreeMap<_, _>>();
    for requested in &request.binding.slots {
        let key = (requested.kind, requested.stable_resource_id.as_str());
        if requested.kind.is_shared_capacity() {
            let capacity = capacities
                .get(&key)
                .and_then(|allocation| allocation.scalar_amount())
                .ok_or_else(|| {
                    ResourceClaimExecutionError::Conflict(format!(
                        "shared resource slot {} has no current scalar Agent capacity",
                        requested.stable_resource_id
                    ))
                })?;
            let requested_amount = requested.allocation.scalar_amount().ok_or_else(|| {
                ResourceClaimExecutionError::Invalid(format!(
                    "shared resource slot {} request is not scalar",
                    requested.stable_resource_id
                ))
            })?;
            let active_amount = active
                .iter()
                .flat_map(|binding| &binding.slots)
                .filter(|slot| {
                    slot.kind == requested.kind
                        && slot.stable_resource_id == requested.stable_resource_id
                })
                .try_fold(0_u64, |total, slot| {
                    let amount = slot.allocation.scalar_amount().ok_or_else(|| {
                        ResourceClaimExecutionError::Invalid(
                            "durable shared resource claim allocation is not scalar".into(),
                        )
                    })?;
                    total.checked_add(amount).ok_or_else(|| {
                        ResourceClaimExecutionError::Invalid(
                            "durable shared resource allocation total overflowed".into(),
                        )
                    })
                })?;
            if active_amount
                .checked_add(requested_amount)
                .is_none_or(|required| required > capacity)
            {
                return Err(ResourceClaimExecutionError::Conflict(format!(
                    "shared resource slot {} has insufficient current Agent capacity",
                    requested.stable_resource_id
                )));
            }
        } else if active.iter().any(|binding| {
            binding.slots.iter().any(|slot| {
                slot.kind == requested.kind
                    && slot.stable_resource_id == requested.stable_resource_id
            })
        }) {
            return Err(ResourceClaimExecutionError::Conflict(format!(
                "exclusive resource slot {} is already prepared on the Agent",
                requested.stable_resource_id
            )));
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ResourceClaimExecutionError {
    #[error("invalid resource claim command: {0}")]
    Invalid(String),
    #[error("resource claim command conflict: {0}")]
    Conflict(String),
    #[error("resource inventory unavailable: {0}")]
    Inventory(#[from] ResourceInventoryError),
    #[error(transparent)]
    Journal(#[from] CommandJournalError),
    #[error("resource claim authority is not configured")]
    AuthorityUnavailable,
}

impl ResourceClaimExecutionError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_resource_claim",
            Self::Conflict(_) => "resource_claim_conflict",
            Self::Inventory(_) => "resource_inventory_unavailable",
            Self::Journal(_) => "resource_claim_journal",
            Self::AuthorityUnavailable => "resource_claim_authority_unavailable",
        }
    }

    pub(crate) fn retryable(&self) -> bool {
        match self {
            Self::Inventory(error) => error.retryable(),
            Self::Journal(CommandJournalError::Storage(_)) => true,
            Self::Invalid(_)
            | Self::Conflict(_)
            | Self::Journal(CommandJournalError::Invalid(_) | CommandJournalError::Conflict(_))
            | Self::AuthorityUnavailable => false,
        }
    }
}
