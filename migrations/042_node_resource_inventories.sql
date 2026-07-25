create table node_resource_inventories (
    organization_id uuid not null,
    node_id uuid not null,
    generation bigint not null check (generation > 0),
    inventory_digest text not null check (
        inventory_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    agent_instance_id uuid not null check (
        agent_instance_id <> '00000000-0000-0000-0000-000000000000'
    ),
    observed_at timestamptz not null,
    received_at timestamptz not null,
    snapshot jsonb not null,
    primary key (node_id, generation),
    unique (node_id, generation, inventory_digest),
    unique (organization_id, node_id, generation),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (
        jsonb_typeof(snapshot) = 'object'
        and snapshot ->> 'schema' =
            'a3s.cloud.node-resource-inventory.v1'
        and snapshot ->> 'node_id' = node_id::text
        and snapshot ->> 'agent_instance_id' = agent_instance_id::text
        and (snapshot ->> 'generation')::bigint = generation
        and snapshot ->> 'digest' = inventory_digest
        and jsonb_typeof(snapshot -> 'slots') = 'array'
        and jsonb_array_length(snapshot -> 'slots') between 1 and 256
    )
);

create table node_resource_inventory_slots (
    organization_id uuid not null,
    node_id uuid not null,
    inventory_generation bigint not null check (inventory_generation > 0),
    ordinal integer not null check (ordinal between 0 and 255),
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
    primary key (node_id, inventory_generation, ordinal),
    unique (
        node_id,
        inventory_generation,
        resource_kind,
        stable_resource_id
    ),
    foreign key (organization_id, node_id, inventory_generation)
        references node_resource_inventories (
            organization_id,
            node_id,
            generation
        ) on delete cascade,
    check (length(stable_resource_id) between 1 and 255),
    check (stable_resource_id = btrim(stable_resource_id)),
    check (
        position(chr(13) in stable_resource_id) = 0
        and position(chr(10) in stable_resource_id) = 0
        and position(chr(9) in stable_resource_id) = 0
    )
);

create table node_resource_inventory_heads (
    organization_id uuid not null,
    node_id uuid primary key,
    generation bigint not null check (generation > 0),
    inventory_digest text not null check (
        inventory_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    agent_instance_id uuid not null check (
        agent_instance_id <> '00000000-0000-0000-0000-000000000000'
    ),
    observed_at timestamptz not null,
    received_at timestamptz not null,
    unique (organization_id, node_id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (node_id, generation, inventory_digest)
        references node_resource_inventories (
            node_id,
            generation,
            inventory_digest
        )
);

create index node_resource_inventory_slots_capacity_idx
    on node_resource_inventory_slots (
        resource_kind,
        stable_resource_id,
        node_id,
        inventory_generation
    );
