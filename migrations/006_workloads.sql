create table workloads (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    name text not null,
    name_key text not null,
    desired_state text not null check (desired_state in ('running', 'stopped')),
    active_revision_id uuid,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, id),
    unique (organization_id, environment_id, name_key),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (updated_at >= created_at)
);

create table workload_revisions (
    id uuid primary key,
    workload_id uuid not null references workloads(id),
    generation bigint not null check (generation > 0),
    artifact_uri text not null,
    artifact_digest text not null,
    artifact_media_type text not null,
    template jsonb not null,
    template_digest text not null,
    created_at timestamptz not null,
    unique (workload_id, generation),
    unique (workload_id, id),
    check (artifact_uri like 'oci://%@sha256:%'),
    check (artifact_digest ~ '^sha256:[0-9a-f]{64}$'),
    check (template_digest ~ '^sha256:[0-9a-f]{64}$')
);

alter table workloads
    add constraint workloads_active_revision_fk
    foreign key (id, active_revision_id)
    references workload_revisions (workload_id, id);

alter table operation_requests
    add constraint operation_requests_organization_id_operation_id_unique
    unique (organization_id, operation_id);

alter table node_commands
    add constraint node_commands_node_id_id_unique
    unique (node_id, id);

create table deployments (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    workload_id uuid not null,
    revision_id uuid not null,
    operation_id uuid not null unique,
    node_id uuid,
    command_id uuid,
    status text not null check (
        status in (
            'queued', 'resolving', 'scheduled', 'applying', 'verifying',
            'active', 'failed', 'cancelled'
        )
    ),
    failure text,
    aggregate_version bigint not null check (aggregate_version > 0),
    requested_at timestamptz not null,
    updated_at timestamptz not null,
    activated_at timestamptz,
    unique (workload_id, revision_id),
    foreign key (organization_id, workload_id)
        references workloads (organization_id, id),
    foreign key (organization_id, operation_id)
        references operation_requests (organization_id, operation_id),
    foreign key (workload_id, revision_id)
        references workload_revisions (workload_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (node_id, command_id)
        references node_commands (node_id, id),
    check (updated_at >= requested_at),
    check ((status = 'failed') = (failure is not null)),
    check ((status = 'active') = (activated_at is not null)),
    check (status in ('queued', 'resolving', 'failed', 'cancelled') or node_id is not null),
    check (status in ('queued', 'resolving', 'scheduled', 'failed', 'cancelled') or command_id is not null)
);

create index deployments_reconcile_idx
    on deployments (status, updated_at, id)
    where status not in ('active', 'failed', 'cancelled');

create index deployments_workload_time_idx
    on deployments (organization_id, workload_id, requested_at desc, id desc);
