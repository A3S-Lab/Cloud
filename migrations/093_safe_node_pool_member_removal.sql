alter table node_pools
    add column member_removal_generation bigint not null default 0
        check (member_removal_generation >= 0);

alter table node_pool_members
    add column removal_generation bigint,
    add column removal_requested_at timestamptz,
    add constraint node_pool_members_removal_state_check check (
        (
            removal_generation is null
            and removal_requested_at is null
        )
        or
        (
            removal_generation > 0
            and removal_requested_at is not null
            and removal_requested_at >= joined_at
        )
    );

create index node_pool_members_pending_removal_idx
    on node_pool_members (organization_id, node_pool_id, removal_generation, node_id)
    where removal_generation is not null;

create index workload_replica_members_node_placement_idx
    on workload_replica_members (organization_id, node_id, id)
    where node_id is not null;

create index resource_claims_active_node_idx
    on resource_claims (organization_id, node_id, id)
    where state <> 'released';
