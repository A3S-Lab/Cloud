create table resource_claims (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    deployment_id uuid not null references deployment_replica_bindings(deployment_id),
    replica_id uuid not null,
    replica_generation bigint not null check (replica_generation > 0),
    member_id uuid not null,
    placement_generation bigint not null check (placement_generation > 0),
    node_id uuid not null,
    inventory_generation bigint not null check (inventory_generation > 0),
    inventory_digest text not null check (
        inventory_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    runtime_unit_id text not null,
    runtime_generation bigint not null check (runtime_generation > 0),
    topology_digest text not null check (
        topology_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    reservation_digest text not null check (
        reservation_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    claim_generation bigint not null check (claim_generation > 0),
    claim_digest text not null check (
        claim_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    state text not null check (
        state in (
            'reserved_in_db',
            'preparing_on_agent',
            'prepared_on_agent',
            'bound_to_runtime_unit',
            'releasing',
            'released',
            'orphaned'
        )
    ),
    prepare_command_id uuid,
    prepared_binding_digest text check (
        prepared_binding_digest is null
        or prepared_binding_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    release_command_id uuid,
    release_evidence jsonb,
    failure text,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    prepared_at timestamptz,
    bound_at timestamptz,
    release_requested_at timestamptz,
    released_at timestamptz,
    orphaned_at timestamptz,
    unique (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, workload_id, replica_id)
        references workload_replicas (organization_id, workload_id, id),
    foreign key (workload_id, replica_id, member_id)
        references workload_replica_members (workload_id, replica_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (runtime_generation = replica_generation),
    check (length(runtime_unit_id) between 1 and 512),
    check (
        position(chr(13) in runtime_unit_id) = 0
        and position(chr(10) in runtime_unit_id) = 0
    ),
    check (updated_at >= created_at),
    check (prepared_at is null or prepared_at >= created_at),
    check (bound_at is null or bound_at >= created_at),
    check (release_requested_at is null or release_requested_at >= created_at),
    check (released_at is null or released_at >= created_at),
    check (orphaned_at is null or orphaned_at >= created_at),
    check (release_evidence is null or jsonb_typeof(release_evidence) = 'object'),
    check (
        (state = 'reserved_in_db') = (
            prepare_command_id is null
            and prepared_binding_digest is null
            and prepared_at is null
            and bound_at is null
            and release_command_id is null
            and release_requested_at is null
            and release_evidence is null
            and released_at is null
            and failure is null
            and orphaned_at is null
        )
        or state <> 'reserved_in_db'
    ),
    check (
        state <> 'preparing_on_agent'
        or (
            prepare_command_id is not null
            and prepared_binding_digest is null
            and prepared_at is null
            and bound_at is null
            and release_command_id is null
            and release_evidence is null
            and failure is null
        )
    ),
    check (
        state <> 'prepared_on_agent'
        or (
            prepare_command_id is not null
            and prepared_binding_digest is not null
            and prepared_at is not null
            and bound_at is null
            and release_command_id is null
            and release_evidence is null
            and failure is null
        )
    ),
    check (
        state <> 'bound_to_runtime_unit'
        or (
            prepare_command_id is not null
            and prepared_binding_digest is not null
            and prepared_at is not null
            and bound_at is not null
            and release_command_id is null
            and release_evidence is null
            and failure is null
        )
    ),
    check (
        state <> 'releasing'
        or (
            release_command_id is not null
            and release_requested_at is not null
            and release_evidence is null
            and released_at is null
        )
    ),
    check (
        state <> 'released'
        or (
            release_evidence is not null
            and released_at is not null
            and failure is null
        )
    ),
    check (
        state <> 'orphaned'
        or (
            failure is not null
            and orphaned_at is not null
            and release_evidence is null
            and released_at is null
        )
    )
);

create table resource_claim_slots (
    claim_id uuid not null,
    ordinal integer not null check (ordinal between 0 and 255),
    organization_id uuid not null,
    node_id uuid not null,
    resource_kind text not null check (
        resource_kind in (
            'cpu',
            'memory',
            'ephemeral_storage',
            'host_port',
            'accelerator',
            'volume'
        )
    ),
    stable_resource_id text not null,
    allocation jsonb not null check (jsonb_typeof(allocation) = 'object'),
    slot_generation bigint not null check (slot_generation > 0),
    fence_token uuid not null check (
        fence_token <> '00000000-0000-0000-0000-000000000000'
    ),
    created_at timestamptz not null,
    released_at timestamptz,
    primary key (claim_id, ordinal),
    unique (claim_id, resource_kind, stable_resource_id),
    unique (
        organization_id,
        node_id,
        resource_kind,
        stable_resource_id,
        slot_generation
    ),
    foreign key (organization_id, claim_id)
        references resource_claims (organization_id, id) on delete cascade,
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (length(stable_resource_id) between 1 and 255),
    check (stable_resource_id = btrim(stable_resource_id)),
    check (
        position(chr(13) in stable_resource_id) = 0
        and position(chr(10) in stable_resource_id) = 0
        and position(chr(9) in stable_resource_id) = 0
    ),
    check (released_at is null or released_at >= created_at)
);

create unique index resource_claim_slots_one_active_slot_idx
    on resource_claim_slots (
        organization_id,
        node_id,
        resource_kind,
        stable_resource_id
    )
    where released_at is null;

create table resource_slot_leases (
    organization_id uuid not null,
    node_id uuid not null,
    resource_kind text not null check (
        resource_kind in (
            'cpu',
            'memory',
            'ephemeral_storage',
            'host_port',
            'accelerator',
            'volume'
        )
    ),
    stable_resource_id text not null,
    slot_generation bigint not null check (slot_generation > 0),
    fence_token uuid not null check (
        fence_token <> '00000000-0000-0000-0000-000000000000'
    ),
    active_claim_id uuid,
    updated_at timestamptz not null,
    primary key (
        organization_id,
        node_id,
        resource_kind,
        stable_resource_id
    ),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (organization_id, active_claim_id)
        references resource_claims (organization_id, id)
        deferrable initially deferred,
    check (length(stable_resource_id) between 1 and 255),
    check (stable_resource_id = btrim(stable_resource_id)),
    check (
        position(chr(13) in stable_resource_id) = 0
        and position(chr(10) in stable_resource_id) = 0
        and position(chr(9) in stable_resource_id) = 0
    )
);

create unique index resource_claims_one_active_replica_member_idx
    on resource_claims (
        organization_id,
        replica_id,
        replica_generation,
        member_id
    )
    where state <> 'released';

create index resource_claims_reconcile_idx
    on resource_claims (state, updated_at, id)
    where state <> 'released';

create index resource_claims_runtime_idx
    on resource_claims (
        organization_id,
        runtime_unit_id,
        runtime_generation
    );
