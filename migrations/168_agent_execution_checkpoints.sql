create table agent_execution_checkpoints (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    conversation_id uuid not null,
    execution_id uuid not null,
    id uuid not null,
    through_event_sequence bigint not null check (through_event_sequence > 0),
    event_count integer not null check (event_count between 1 and 1000),
    agent_artifact_digest text not null check (
        agent_artifact_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    provider_profile_digest text not null check (
        provider_profile_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    invocation_profile_digest text not null check (
        invocation_profile_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    object_schema text not null check (
        object_schema = 'a3s.cloud.agent-execution-checkpoint-object.v1'
    ),
    object_namespace text not null check (object_namespace = 'agent-checkpoints'),
    object_ref text not null check (
        octet_length(object_ref) between 1 and 4096
        and position(chr(92) in object_ref) = 0
        and position(chr(13) in object_ref) = 0
        and position(chr(10) in object_ref) = 0
    ),
    object_digest text not null check (object_digest ~ '^sha256:[0-9a-f]{64}$'),
    object_size_bytes bigint not null check (object_size_bytes between 1 and 917504),
    object_media_type text not null check (
        object_media_type = 'application/vnd.a3s.agent-execution-checkpoint+json;version=1'
    ),
    operation_id uuid not null,
    provider_run_identity_digest text not null check (
        provider_run_identity_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    node_id uuid not null,
    workload_id uuid not null,
    workload_revision_id uuid not null,
    deployment_id uuid not null,
    replica_id uuid not null,
    runtime_unit_id text not null check (
        octet_length(runtime_unit_id) between 1 and 512
        and btrim(runtime_unit_id) = runtime_unit_id
        and runtime_unit_id !~ E'[\r\n]'
    ),
    runtime_generation bigint not null check (runtime_generation > 0),
    aggregate_version bigint not null check (aggregate_version = 1),
    captured_at timestamptz not null,
    check (
        object_ref = 'organizations/' || organization_id::text
            || '/executions/' || execution_id::text
            || '/checkpoints/' || id::text
            || '/sha256/' || substring(object_digest from 8)
            || '/checkpoint.json'
    ),
    primary key (organization_id, id),
    unique (organization_id, execution_id, through_event_sequence),
    unique (organization_id, execution_id, id, object_digest),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, conversation_id, execution_id)
        references agent_executions (organization_id, conversation_id, id)
);

create index agent_execution_checkpoints_execution_sequence_idx
    on agent_execution_checkpoints (
        organization_id,
        execution_id,
        through_event_sequence desc,
        id desc
    );

alter table agent_executions
    add column parent_execution_id uuid,
    add column parent_checkpoint_id uuid,
    add column parent_checkpoint_digest text,
    add column fork_depth integer,
    add constraint agent_executions_fork_lineage_shape check (
        (
            parent_execution_id is null
            and parent_checkpoint_id is null
            and parent_checkpoint_digest is null
            and fork_depth is null
        )
        or (
            parent_execution_id is not null
            and parent_checkpoint_id is not null
            and parent_checkpoint_digest is not null
            and parent_checkpoint_digest ~ '^sha256:[0-9a-f]{64}$'
            and fork_depth is not null
            and fork_depth between 1 and 64
            and parent_execution_id <> id
        )
    ),
    add constraint agent_executions_parent_execution_fk
        foreign key (organization_id, conversation_id, parent_execution_id)
        references agent_executions (organization_id, conversation_id, id),
    add constraint agent_executions_parent_checkpoint_fk
        foreign key (
            organization_id,
            parent_execution_id,
            parent_checkpoint_id,
            parent_checkpoint_digest
        )
        references agent_execution_checkpoints (
            organization_id,
            execution_id,
            id,
            object_digest
        );

create index agent_executions_parent_lineage_idx
    on agent_executions (organization_id, parent_execution_id, parent_checkpoint_id)
    where parent_execution_id is not null;

comment on table agent_execution_checkpoints is
    'A1.6 immutable logical Agent trajectory checkpoints; object bytes remain in the shared immutable-object authority';

comment on column agent_executions.parent_checkpoint_id is
    'A1.6 immutable logical fork lineage; a fork is a new execution and never mutates its parent trajectory';
