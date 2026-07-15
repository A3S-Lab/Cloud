create table gateway_scopes (
    node_id uuid primary key references nodes(id),
    last_issued_revision bigint not null check (last_issued_revision > 0),
    installed_revision bigint check (
        installed_revision is null
        or installed_revision > 0 and installed_revision <= last_issued_revision
    ),
    aggregate_version bigint not null check (aggregate_version > 0),
    updated_at timestamptz not null
);

create table gateway_publications (
    node_id uuid not null references nodes(id),
    revision bigint not null check (revision > 0),
    expected_revision bigint,
    command_id uuid not null,
    command_correlation_id uuid not null,
    snapshot_digest text not null check (snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    acl text not null check (octet_length(acl) between 1 and 1048576),
    state text not null check (state in ('pending', 'applied', 'rejected')),
    failure text,
    command_issued_at timestamptz not null,
    command_not_after timestamptz not null,
    acknowledged_at timestamptz,
    primary key (node_id, revision),
    unique (node_id, command_id),
    unique (node_id, revision, command_id),
    check (expected_revision is null or expected_revision > 0 and expected_revision < revision),
    check (command_not_after > command_issued_at),
    check (
        state = 'pending' and failure is null and acknowledged_at is null
        or state = 'applied' and failure is null and acknowledged_at is not null
        or state = 'rejected' and failure is not null and acknowledged_at is not null
    )
);

create unique index gateway_publications_one_pending_idx
    on gateway_publications (node_id)
    where state = 'pending';

create table routes (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    gateway_node_id uuid not null,
    hostname text not null,
    path_prefix text not null,
    workload_id uuid not null,
    workload_revision_id uuid not null,
    port_name text not null,
    upstream_origin text not null,
    state text not null check (state in ('publishing', 'active', 'rejected')),
    gateway_revision bigint not null check (gateway_revision > 0),
    gateway_command_id uuid not null,
    snapshot_digest text not null check (snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    failure text,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    activated_at timestamptz,
    unique (gateway_node_id, hostname, path_prefix),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, workload_id)
        references workloads (organization_id, id),
    foreign key (workload_id, workload_revision_id)
        references workload_revisions (workload_id, id),
    foreign key (organization_id, gateway_node_id)
        references nodes (organization_id, id),
    foreign key (gateway_node_id, gateway_revision, gateway_command_id)
        references gateway_publications (node_id, revision, command_id),
    check (updated_at >= created_at),
    check (
        state = 'publishing' and failure is null and activated_at is null
        or state = 'active' and failure is null and activated_at is not null
        or state = 'rejected' and failure is not null and activated_at is null
    )
);

create index routes_environment_idx
    on routes (organization_id, project_id, environment_id, created_at, id);

create index routes_gateway_state_idx
    on routes (gateway_node_id, state, hostname, path_prefix, id);
