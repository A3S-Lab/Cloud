create schema if not exists a3s_flow;

create table operation_requests (
    operation_id uuid primary key,
    organization_id uuid not null references organizations(id),
    subject_kind text not null,
    subject_id uuid not null,
    workflow_name text not null,
    workflow_version text not null,
    input jsonb not null,
    requested_at timestamptz not null,
    check (subject_kind ~ '^[a-z][a-z0-9_]{0,62}$'),
    check (length(workflow_name) between 1 and 255),
    check (length(workflow_version) between 1 and 63)
);

create index operation_requests_organization_time_idx
    on operation_requests (organization_id, requested_at desc, operation_id desc);

create table operation_projections (
    operation_id uuid primary key references operation_requests(operation_id) on delete cascade,
    status text not null check (
        status in ('queued', 'running', 'suspended', 'succeeded', 'failed', 'cancelled')
    ),
    last_sequence bigint not null check (last_sequence >= 0),
    output jsonb,
    error text,
    updated_at timestamptz not null
);

create index operation_projections_status_idx
    on operation_projections (status, updated_at, operation_id);
