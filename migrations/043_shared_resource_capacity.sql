drop index resource_claim_slots_one_active_slot_idx;

update resource_slot_leases
set active_claim_id = null
where resource_kind in ('cpu', 'memory', 'ephemeral_storage');

create unique index resource_claim_slots_one_active_exclusive_slot_idx
    on resource_claim_slots (
        organization_id,
        node_id,
        resource_kind,
        stable_resource_id
    )
    where released_at is null
      and resource_kind in ('host_port', 'accelerator', 'volume');
