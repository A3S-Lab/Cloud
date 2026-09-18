-- DirectoryProjection Resource Grants: opaque department/group subject refs.
-- Cloud owns Grants; partner directory trees are not stored.
-- membership_id is intentionally absent (separate from membership-scoped resource_grants).

create table directory_resource_grants (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    subject_kind text not null check (subject_kind in ('department', 'group')),
    directory_issuer text not null,
    directory_subject_id uuid not null,
    scope_kind text not null check (scope_kind in ('project', 'environment', 'application', 'node')),
    project_id uuid,
    environment_id uuid,
    application_id uuid,
    node_id uuid,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    revoked_at timestamptz,
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, project_id, application_id)
        references applications (organization_id, project_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (updated_at >= created_at),
    check (revoked_at is null or revoked_at = updated_at),
    check (
        (scope_kind = 'project'
            and project_id is not null
            and environment_id is null
            and application_id is null
            and node_id is null)
        or (scope_kind = 'environment'
            and project_id is not null
            and environment_id is not null
            and application_id is null
            and node_id is null)
        or (scope_kind = 'application'
            and project_id is not null
            and environment_id is null
            and application_id is not null
            and node_id is null)
        or (scope_kind = 'node'
            and project_id is null
            and environment_id is null
            and application_id is null
            and node_id is not null)
    )
);

create unique index directory_resource_grants_active_project_idx
    on directory_resource_grants (
        organization_id,
        subject_kind,
        directory_issuer,
        directory_subject_id,
        project_id
    )
    where scope_kind = 'project' and revoked_at is null;

create unique index directory_resource_grants_active_environment_idx
    on directory_resource_grants (
        organization_id,
        subject_kind,
        directory_issuer,
        directory_subject_id,
        project_id,
        environment_id
    )
    where scope_kind = 'environment' and revoked_at is null;

create unique index directory_resource_grants_active_application_idx
    on directory_resource_grants (
        organization_id,
        subject_kind,
        directory_issuer,
        directory_subject_id,
        project_id,
        application_id
    )
    where scope_kind = 'application' and revoked_at is null;

create unique index directory_resource_grants_active_node_idx
    on directory_resource_grants (
        organization_id,
        subject_kind,
        directory_issuer,
        directory_subject_id,
        node_id
    )
    where scope_kind = 'node' and revoked_at is null;

create index directory_resource_grants_org_history_idx
    on directory_resource_grants (organization_id, created_at, id);

create index directory_resource_grants_active_scope_idx
    on directory_resource_grants (
        organization_id,
        scope_kind,
        project_id,
        environment_id,
        application_id,
        node_id
    )
    where revoked_at is null;

create index directory_resource_grants_active_subject_idx
    on directory_resource_grants (
        organization_id,
        subject_kind,
        directory_issuer,
        directory_subject_id
    )
    where revoked_at is null;
