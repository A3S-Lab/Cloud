-- Tenant Plugin Registry enrollment retains trust anchors, not TUF metadata.
create table plugin_registries (
    organization_id uuid not null references organizations(id) check (
        organization_id <> '00000000-0000-0000-0000-000000000000'
    ),
    id uuid primary key check (
        id <> '00000000-0000-0000-0000-000000000000'
    ),
    name text not null check (char_length(name) between 1 and 63),
    name_key text not null check (char_length(name_key) between 1 and 63),
    endpoint text not null check (
        char_length(endpoint) between 9 and 2048
        and endpoint like 'https://%/'
    ),
    root_object_ref text not null check (
        root_object_ref ~ '^sha256/[0-9a-f]{64}/root[.]json$'
    ),
    root_sha256 text not null check (
        root_sha256 ~ '^sha256:[0-9a-f]{64}$'
    ),
    root_version bigint not null check (root_version > 0),
    state text not null check (state in ('active', 'disabled')),
    aggregate_version bigint not null check (aggregate_version > 0),
    last_actor_id uuid not null references identity_principals(id) check (
        last_actor_id <> '00000000-0000-0000-0000-000000000000'
    ),
    last_request_id uuid not null check (
        last_request_id <> '00000000-0000-0000-0000-000000000000'
    ),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, id),
    unique (organization_id, name_key),
    unique (organization_id, endpoint),
    check (
        root_object_ref = 'sha256/' || substring(root_sha256 from 8) || '/root.json'
    ),
    check (updated_at >= created_at)
);

create index plugin_registries_organization_state_idx
    on plugin_registries (organization_id, state, created_at, id);
