-- U0.3: one live Cloud PluginAssignment per organization, target host, and
-- package. Retains desired catalog selection and Use desired state only; it
-- never stores package bytes, TUF metadata, grants, or host observation.

create table plugin_assignments (
    organization_id uuid not null check (
        organization_id <> '00000000-0000-0000-0000-000000000000'
    ),
    project_id uuid not null check (
        project_id <> '00000000-0000-0000-0000-000000000000'
    ),
    environment_id uuid not null check (
        environment_id <> '00000000-0000-0000-0000-000000000000'
    ),
    id uuid primary key check (
        id <> '00000000-0000-0000-0000-000000000000'
    ),
    registry_id uuid not null check (
        registry_id <> '00000000-0000-0000-0000-000000000000'
    ),
    target_host_id uuid not null check (
        target_host_id <> '00000000-0000-0000-0000-000000000000'
    ),
    workspace_scope jsonb not null,
    package_id text not null check (
        char_length(package_id) between 3 and 127
        and package_id ~ '^[a-z][a-z0-9-]*/[a-z][a-z0-9-]*$'
    ),
    catalog_record_digest text not null check (
        catalog_record_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    version text not null check (char_length(version) between 1 and 128),
    package_digest text not null check (
        package_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    manifest_digest text not null check (
        manifest_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    selected_surfaces jsonb not null,
    policy_digest text not null check (
        policy_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    desired_state text not null check (
        desired_state in ('enabled', 'installed-disabled', 'absent')
    ),
    assignment_generation bigint not null check (assignment_generation > 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    current_operation_id uuid check (
        current_operation_id is null
        or current_operation_id <> '00000000-0000-0000-0000-000000000000'
    ),
    last_actor_id uuid not null references identity_principals(id) check (
        last_actor_id <> '00000000-0000-0000-0000-000000000000'
    ),
    last_request_id uuid not null check (
        last_request_id <> '00000000-0000-0000-0000-000000000000'
    ),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, id),
    unique (organization_id, target_host_id, package_id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, registry_id)
        references plugin_registries (organization_id, id),
    foreign key (organization_id, target_host_id)
        references nodes (organization_id, id),
    check (updated_at >= created_at),
    check (jsonb_typeof(workspace_scope) = 'object'),
    check (jsonb_typeof(selected_surfaces) = 'array'),
    check (jsonb_array_length(selected_surfaces) between 1 and 256)
);

create index plugin_assignments_environment_idx
    on plugin_assignments (
        organization_id,
        environment_id,
        created_at,
        id
    );

create index plugin_assignments_host_package_idx
    on plugin_assignments (
        organization_id,
        target_host_id,
        package_id
    );
