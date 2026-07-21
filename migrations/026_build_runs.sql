alter table external_source_revisions
    add constraint external_source_revisions_full_identity_unique
    unique (organization_id, project_id, environment_id, id);

create table build_runs (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    source_revision_id uuid not null,
    operation_id uuid not null unique,
    status text not null check (
        status in (
            'queued',
            'preparing',
            'prepared',
            'scheduled',
            'running',
            'validating',
            'cancelling',
            'cleanup_pending',
            'succeeded',
            'failed',
            'cancelled'
        )
    ),
    source_content_digest text,
    input_artifact jsonb,
    node_id uuid,
    command_id uuid,
    cleanup_command_id uuid,
    runtime_spec_digest text,
    runtime_output_artifact jsonb,
    output jsonb,
    failure text,
    aggregate_version bigint not null check (aggregate_version > 0),
    requested_at timestamptz not null,
    updated_at timestamptz not null,
    started_at timestamptz,
    cancellation_requested_at timestamptz,
    finished_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, source_revision_id),
    foreign key (
        organization_id,
        project_id,
        environment_id,
        source_revision_id
    ) references external_source_revisions (
        organization_id,
        project_id,
        environment_id,
        id
    ),
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
        (source_content_digest is null and input_artifact is null)
        or (
            source_content_digest ~ '^sha256:[0-9a-f]{64}$'
            and jsonb_typeof(input_artifact) = 'object'
        )
    ),
    check (
        runtime_spec_digest is null
        or runtime_spec_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    check (input_artifact is null or started_at is not null),
    check ((node_id is null) = (runtime_spec_digest is null)),
    check (node_id is null or input_artifact is not null),
    check (command_id is null or node_id is not null),
    check (cleanup_command_id is null or command_id is not null),
    check (
        runtime_output_artifact is null
        or (
            command_id is not null
            and jsonb_typeof(runtime_output_artifact) = 'object'
        )
    ),
    check (
        output is null
        or (
            runtime_output_artifact is not null
            and jsonb_typeof(output) = 'object'
            and output -> 'artifact' = runtime_output_artifact
        )
    ),
    check (failure is null or octet_length(failure) between 1 and 16384),
    check (
        status <> 'queued'
        or (
            started_at is null
            and source_content_digest is null
            and node_id is null
            and command_id is null
            and cleanup_command_id is null
            and runtime_output_artifact is null
            and output is null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'preparing'
        or (
            started_at is not null
            and source_content_digest is null
            and node_id is null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'prepared'
        or (
            started_at is not null
            and input_artifact is not null
            and node_id is null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'scheduled'
        or (
            input_artifact is not null
            and node_id is not null
            and command_id is null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'running'
        or (
            command_id is not null
            and runtime_output_artifact is null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'validating'
        or (
            command_id is not null
            and runtime_output_artifact is not null
            and cleanup_command_id is null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'cancelling'
        or cancellation_requested_at is not null
    ),
    check (
        status <> 'cleanup_pending'
        or output is not null
        or failure is not null
        or cancellation_requested_at is not null
    ),
    check (
        status <> 'succeeded'
        or (
            output is not null
            and cleanup_command_id is not null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'failed'
        or (
            failure is not null
            and cancellation_requested_at is null
        )
    ),
    check (
        status <> 'cancelled'
        or cancellation_requested_at is not null
    ),
    check (
        status not in ('succeeded', 'failed', 'cancelled')
        or command_id is null
        or cleanup_command_id is not null
    )
);

create index build_runs_environment_time_idx
    on build_runs (
        organization_id,
        project_id,
        environment_id,
        requested_at,
        id
    );

create index build_runs_reconciliation_idx
    on build_runs (status, requested_at, id)
    where status not in ('succeeded', 'failed', 'cancelled');
