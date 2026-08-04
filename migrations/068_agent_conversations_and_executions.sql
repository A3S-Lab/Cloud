create table agent_conversations (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    status text not null check (status in ('active', 'closed')),
    last_event_sequence bigint not null check (last_event_sequence >= 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    closed_at timestamptz,
    primary key (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (updated_at >= created_at),
    check (
        (status = 'active' and closed_at is null)
        or (
            status = 'closed'
            and closed_at is not null
            and closed_at = updated_at
        )
    )
);

create index agent_conversations_environment_time_idx
    on agent_conversations (
        organization_id,
        project_id,
        environment_id,
        created_at desc,
        id desc
    );

create table agent_executions (
    organization_id uuid not null,
    conversation_id uuid not null,
    id uuid not null,
    operation_id uuid not null unique,
    agent_asset_id uuid not null,
    agent_asset_release_id uuid not null,
    agent_build_run_id uuid not null,
    agent_artifact_uri text not null check (
        length(agent_artifact_uri) between 1 and 2048
        and agent_artifact_uri like 'oci://%'
    ),
    agent_artifact_digest text not null check (
        agent_artifact_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    agent_artifact_media_type text not null check (
        length(agent_artifact_media_type) between 1 and 255
    ),
    agent_artifact_size_bytes bigint not null check (
        agent_artifact_size_bytes > 0
    ),
    status text not null check (
        status in ('pending', 'running', 'succeeded', 'failed', 'cancelled')
    ),
    failure text check (octet_length(failure) between 1 and 16384),
    aggregate_version bigint not null check (aggregate_version > 0),
    requested_at timestamptz not null,
    updated_at timestamptz not null,
    started_at timestamptz,
    finished_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, conversation_id, id),
    foreign key (organization_id, conversation_id)
        references agent_conversations (organization_id, id),
    foreign key (
        organization_id,
        agent_asset_id,
        agent_asset_release_id,
        agent_build_run_id,
        agent_artifact_digest,
        agent_artifact_media_type
    ) references asset_releases (
        organization_id,
        asset_id,
        id,
        build_run_id,
        artifact_digest,
        artifact_media_type
    ),
    foreign key (
        organization_id,
        agent_asset_id,
        agent_asset_release_id,
        agent_artifact_digest,
        agent_artifact_media_type,
        agent_artifact_size_bytes
    ) references asset_releases (
        organization_id,
        asset_id,
        id,
        artifact_digest,
        artifact_media_type,
        artifact_size_bytes
    ),
    check (
        agent_artifact_uri like '%@' || agent_artifact_digest
    ),
    check (updated_at >= requested_at),
    check (started_at is null or started_at >= requested_at),
    check (finished_at is null or finished_at >= requested_at),
    check (status <> 'running' or started_at is not null),
    check (
        (status in ('succeeded', 'failed', 'cancelled'))
        = (finished_at is not null)
    ),
    check ((status = 'failed') = (failure is not null))
);

create index agent_executions_conversation_time_idx
    on agent_executions (
        organization_id,
        conversation_id,
        requested_at desc,
        id desc
    );

create index agent_executions_release_idx
    on agent_executions (
        organization_id,
        agent_asset_id,
        agent_asset_release_id,
        requested_at,
        id
    );

create table agent_execution_events (
    organization_id uuid not null,
    conversation_id uuid not null,
    sequence bigint not null check (sequence > 0),
    execution_id uuid not null,
    kind text not null check (
        kind in (
            'execution_requested',
            'model_output',
            'execution_failed',
            'execution_completed'
        )
    ),
    content jsonb not null,
    content_digest text not null check (
        content_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    content_size_bytes bigint not null check (
        content_size_bytes between 1 and 65536
    ),
    occurred_at timestamptz not null,
    primary key (organization_id, conversation_id, sequence),
    foreign key (organization_id, conversation_id)
        references agent_conversations (organization_id, id),
    foreign key (organization_id, conversation_id, execution_id)
        references agent_executions (organization_id, conversation_id, id)
);

create index agent_execution_events_execution_sequence_idx
    on agent_execution_events (
        organization_id,
        execution_id,
        sequence
    );

comment on table agent_conversations is
    'A1 tenant-scoped Agent conversations and the sole semantic event-stream head';

comment on table agent_executions is
    'A1 logical Agent runs pinned to one exact immutable published Agent release';

comment on table agent_execution_events is
    'A1 immutable, contiguous semantic conversation history distinct from Flow history and Runtime logs';
