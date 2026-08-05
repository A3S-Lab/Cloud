alter table deployment_replica_bindings
    add constraint deployment_replica_bindings_code_run_identity_unique
    unique (
        organization_id,
        deployment_id,
        workload_id,
        revision_id,
        replica_id,
        node_id,
        runtime_unit_id,
        runtime_generation
    );

alter table agent_executions
    add column code_node_id uuid,
    add column code_workload_id uuid,
    add column code_workload_revision_id uuid,
    add column code_deployment_id uuid,
    add column code_replica_id uuid,
    add column code_runtime_unit_id text,
    add column code_runtime_generation bigint,
    add column code_runtime_spec_digest text,
    add column code_service_port_name text,
    add column code_protocol text,
    add column code_release_identity text,
    add column code_session_id text,
    add column code_run_id text,
    add column code_event_cursor bigint,
    add column code_state text,
    add column code_bound_at timestamptz,
    add column code_observed_at timestamptz,
    add constraint agent_executions_code_runtime_binding_fk
        foreign key (
            organization_id,
            code_deployment_id,
            code_workload_id,
            code_workload_revision_id,
            code_replica_id,
            code_node_id,
            code_runtime_unit_id,
            code_runtime_generation
        ) references deployment_replica_bindings (
            organization_id,
            deployment_id,
            workload_id,
            revision_id,
            replica_id,
            node_id,
            runtime_unit_id,
            runtime_generation
        ),
    add constraint agent_executions_code_binding_complete check (
        (
            code_node_id is null
            and code_workload_id is null
            and code_workload_revision_id is null
            and code_deployment_id is null
            and code_replica_id is null
            and code_runtime_unit_id is null
            and code_runtime_generation is null
            and code_runtime_spec_digest is null
            and code_service_port_name is null
            and code_protocol is null
            and code_release_identity is null
            and code_session_id is null
            and code_run_id is null
            and code_event_cursor is null
            and code_state is null
            and code_bound_at is null
            and code_observed_at is null
        )
        or (
            code_node_id is not null
            and code_workload_id is not null
            and code_workload_revision_id is not null
            and code_deployment_id is not null
            and code_replica_id is not null
            and code_runtime_unit_id is not null
            and code_runtime_generation is not null
            and code_runtime_spec_digest is not null
            and code_service_port_name is not null
            and code_protocol is not null
            and code_release_identity is not null
            and code_session_id is not null
            and code_run_id is not null
            and code_state is not null
            and code_bound_at is not null
        )
    ),
    add constraint agent_executions_code_binding_values check (
        code_node_id is null
        or (
            code_runtime_generation > 0
            and octet_length(code_runtime_unit_id) between 1 and 512
            and btrim(code_runtime_unit_id) <> ''
            and position(chr(13) in code_runtime_unit_id) = 0
            and position(chr(10) in code_runtime_unit_id) = 0
            and code_runtime_spec_digest ~ '^sha256:[0-9a-f]{64}$'
            and octet_length(code_service_port_name) between 1 and 128
            and btrim(code_service_port_name) <> ''
            and position(chr(13) in code_service_port_name) = 0
            and position(chr(10) in code_service_port_name) = 0
            and code_protocol = 'a3s.code.agent.v1'
            and code_release_identity ~ '^sha256:[0-9a-f]{64}$'
            and code_release_identity = agent_artifact_digest
            and octet_length(code_session_id) between 1 and 256
            and octet_length(code_run_id) between 1 and 256
            and btrim(code_session_id) <> ''
            and btrim(code_run_id) <> ''
            and code_session_id !~ E'[\\r\\n]'
            and code_run_id !~ E'[\\r\\n]'
            and (code_event_cursor is null or code_event_cursor >= 0)
            and code_state in (
                'created',
                'planning',
                'executing',
                'verifying',
                'completed',
                'failed',
                'cancelled'
            )
            and code_bound_at >= requested_at
        )
    );

alter table agent_execution_events
    drop constraint agent_execution_events_kind_check,
    add constraint agent_execution_events_kind_check check (
        kind in (
            'execution_requested',
            'model_output',
            'execution_failed',
            'execution_completed',
            'execution_cancelled'
        )
    );

create unique index agent_executions_code_run_identity_unique
    on agent_executions (
        organization_id,
        code_release_identity,
        code_session_id,
        code_run_id
    )
    where code_run_id is not null;

comment on constraint agent_executions_code_runtime_binding_fk
    on agent_executions is
    'A1.2 exact existing Workload replica and Runtime Service hosting the sole a3s code harness process';

comment on column agent_executions.code_event_cursor is
    'Last contiguous A3S Code run-local event sequence committed into the Agent semantic event stream';

comment on column agent_executions.code_observed_at is
    'Source timestamp of the latest accepted page from the sole a3s code harness run store';
