create table node_pools (
    organization_id uuid not null references organizations(id),
    id uuid not null unique,
    name text not null,
    name_key text not null,
    spec_digest text not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    maintenance_generation bigint not null default 0 check (maintenance_generation >= 0),
    maintenance_starts_at timestamptz,
    maintenance_ends_at timestamptz,
    maintenance_reason text,
    maintenance_cancelled_at timestamptz,
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, name_key),
    check (updated_at >= created_at),
    check (spec_digest ~ '^sha256:[0-9a-f]{64}$'),
    check (
        (
            maintenance_generation = 0
            and maintenance_starts_at is null
            and maintenance_ends_at is null
            and maintenance_reason is null
            and maintenance_cancelled_at is null
        )
        or
        (
            maintenance_generation > 0
            and maintenance_starts_at is not null
            and maintenance_ends_at is not null
            and maintenance_ends_at > maintenance_starts_at
            and maintenance_ends_at - maintenance_starts_at <= interval '30 days'
            and maintenance_reason is not null
            and char_length(maintenance_reason) between 1 and 1024
            and position(chr(10) in maintenance_reason) = 0
            and position(chr(13) in maintenance_reason) = 0
            and (
                maintenance_cancelled_at is null
                or maintenance_cancelled_at < maintenance_ends_at
            )
        )
    )
);

create table node_pool_members (
    organization_id uuid not null,
    node_pool_id uuid not null,
    node_id uuid not null,
    joined_at timestamptz not null,
    primary key (organization_id, node_pool_id, node_id),
    unique (organization_id, node_id),
    foreign key (organization_id, node_pool_id)
        references node_pools (organization_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id)
);

create table node_pool_maintenance_targets (
    organization_id uuid not null,
    node_pool_id uuid not null,
    node_id uuid not null,
    primary key (organization_id, node_pool_id, node_id),
    foreign key (organization_id, node_pool_id, node_id)
        references node_pool_members (organization_id, node_pool_id, node_id)
);

create index node_pools_active_maintenance_idx
    on node_pools (maintenance_starts_at, maintenance_ends_at, organization_id, id)
    where maintenance_generation > 0 and maintenance_cancelled_at is null;

create index node_pool_members_pool_idx
    on node_pool_members (organization_id, node_pool_id, node_id);
