create table executions (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    operation_id uuid not null unique,
    template jsonb not null check (jsonb_typeof(template) = 'object'),
    template_digest text not null check (
        template_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    status text not null check (
        status in (
            'queued',
            'scheduled',
            'running',
            'cancelling',
            'cleanup_pending',
            'succeeded',
            'failed',
            'cancelled'
        )
    ),
    node_id uuid,
    command_id uuid,
    cleanup_command_id uuid,
    runtime_spec_digest text check (
        runtime_spec_digest is null
        or runtime_spec_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    outcome jsonb check (
        outcome is null
        or jsonb_typeof(outcome) = 'object'
    ),
    aggregate_version bigint not null check (aggregate_version > 0),
    requested_at timestamptz not null,
    updated_at timestamptz not null,
    started_at timestamptz,
    cancellation_requested_at timestamptz,
    finished_at timestamptz,
    primary key (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (node_id, command_id)
        references node_commands (node_id, id),
    foreign key (node_id, cleanup_command_id)
        references node_commands (node_id, id),
    check (id = operation_id),
    check (updated_at >= requested_at),
    check (started_at is null or started_at >= requested_at),
    check (
        cancellation_requested_at is null
        or cancellation_requested_at >= requested_at
    ),
    check (finished_at is null or finished_at >= requested_at),
    check (
        (status in ('succeeded', 'failed', 'cancelled'))
        = (finished_at is not null)
    ),
    check (
        (status in ('cleanup_pending', 'succeeded', 'failed', 'cancelled'))
        = (outcome is not null)
    ),
    check ((node_id is null) = (runtime_spec_digest is null)),
    check (command_id is null or node_id is not null),
    check (cleanup_command_id is null or node_id is not null)
);

create index executions_environment_requested_idx
    on executions (
        organization_id,
        project_id,
        environment_id,
        requested_at desc,
        id desc
    );

create index executions_pending_operation_idx
    on executions (requested_at, id)
    where status not in ('succeeded', 'failed', 'cancelled');

comment on table executions is
    'Cloud-owned one-shot invocation lifecycle projected to provider-neutral Runtime Tasks';
