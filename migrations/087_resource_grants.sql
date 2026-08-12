alter table organization_memberships
    add constraint organization_memberships_tenant_id_unique
        unique (organization_id, id);

create table resource_grants (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    membership_id uuid not null,
    scope_kind text not null check (scope_kind in ('project', 'environment', 'node')),
    project_id uuid,
    environment_id uuid,
    node_id uuid,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    revoked_at timestamptz,
    foreign key (organization_id, membership_id)
        references organization_memberships (organization_id, id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (updated_at >= created_at),
    check (revoked_at is null or revoked_at = updated_at),
    check (
        (scope_kind = 'project'
            and project_id is not null
            and environment_id is null
            and node_id is null)
        or (scope_kind = 'environment'
            and project_id is not null
            and environment_id is not null
            and node_id is null)
        or (scope_kind = 'node'
            and project_id is null
            and environment_id is null
            and node_id is not null)
    )
);

create unique index resource_grants_active_project_idx
    on resource_grants (organization_id, membership_id, project_id)
    where scope_kind = 'project' and revoked_at is null;

create unique index resource_grants_active_environment_idx
    on resource_grants (organization_id, membership_id, project_id, environment_id)
    where scope_kind = 'environment' and revoked_at is null;

create unique index resource_grants_active_node_idx
    on resource_grants (organization_id, membership_id, node_id)
    where scope_kind = 'node' and revoked_at is null;

create index resource_grants_membership_history_idx
    on resource_grants (organization_id, membership_id, created_at, id);

create index resource_grants_active_scope_idx
    on resource_grants (organization_id, scope_kind, project_id, environment_id, node_id)
    where revoked_at is null;
