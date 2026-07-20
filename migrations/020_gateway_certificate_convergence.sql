alter table routes
    drop constraint routes_gateway_node_id_hostname_path_prefix_key;

create unique index routes_active_ownership_idx
    on routes (gateway_node_id, hostname, path_prefix)
    where state in ('publishing', 'active');

create table gateway_certificate_convergences (
    organization_id uuid not null,
    node_id uuid not null,
    gateway_revision bigint not null check (gateway_revision > 0),
    gateway_command_id uuid not null,
    previous_certificate_id uuid not null references gateway_certificates(id),
    replacement_certificate_id uuid unique references gateway_certificates(id),
    snapshot_digest text not null check (snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    retained_routes jsonb not null,
    rejected_routes jsonb not null,
    reason text not null check (
        reason in ('renewal', 'domain_revocation', 'certificate_revocation', 'projection_repair')
    ),
    state text not null check (state in ('pending', 'applied', 'rejected')),
    failure text,
    staged_at timestamptz not null,
    acknowledged_at timestamptz,
    primary key (node_id, gateway_revision),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (node_id, gateway_revision, gateway_command_id)
        references gateway_publications (node_id, revision, command_id),
    check (jsonb_typeof(retained_routes) = 'array'),
    check (jsonb_typeof(rejected_routes) = 'array'),
    check (jsonb_array_length(retained_routes) + jsonb_array_length(rejected_routes) > 0),
    check (
        jsonb_array_length(retained_routes) = 0 and replacement_certificate_id is null
        or jsonb_array_length(retained_routes) > 0 and replacement_certificate_id is not null
    ),
    check (
        replacement_certificate_id is null
        or replacement_certificate_id <> previous_certificate_id
    ),
    check (
        state = 'pending' and failure is null and acknowledged_at is null
        or state = 'applied' and failure is null and acknowledged_at is not null
        or state = 'rejected' and failure is not null and acknowledged_at is not null
    )
);

create index gateway_certificate_convergences_state_idx
    on gateway_certificate_convergences (state, staged_at, node_id, gateway_revision);
