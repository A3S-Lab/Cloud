alter table agent_executions
    drop constraint agent_executions_status_check,
    add constraint agent_executions_status_check check (
        status in (
            'pending',
            'running',
            'awaiting_approval',
            'cancelling',
            'succeeded',
            'failed',
            'cancelled'
        )
    ),
    drop constraint agent_executions_cancellation_state_check,
    add constraint agent_executions_cancellation_state_check check (
        (status = 'cancelling' and cancellation_requested_at is not null)
        or (
            status in ('pending', 'running', 'awaiting_approval')
            and cancellation_requested_at is null
        )
        or status in ('succeeded', 'failed', 'cancelled')
    ),
    drop constraint agent_executions_provider_binding_values,
    add constraint agent_executions_provider_binding_values check (
        provider_kind ~ '^[a-z0-9]+([.-][a-z0-9]+)*$'
        and octet_length(provider_kind) <= 64
        and octet_length(provider_revision) between 1 and 128
        and provider_revision !~ E'[\r\n]'
        and provider_protocol = 'a3s.cloud.agent-provider.v1'
        and octet_length(provider_native_protocol) between 1 and 128
        and provider_native_protocol !~ E'[\r\n]'
        and octet_length(provider_profile_acl) between 1 and 16384
        and provider_profile_digest ~ '^sha256:[0-9a-f]{64}$'
        and provider_capability_digest ~ '^sha256:[0-9a-f]{64}$'
        and (provider_runtime_generation is null or provider_runtime_generation > 0)
        and (
            provider_runtime_unit_id is null
            or (
                octet_length(provider_runtime_unit_id) between 1 and 512
                and btrim(provider_runtime_unit_id) <> ''
                and position(chr(13) in provider_runtime_unit_id) = 0
                and position(chr(10) in provider_runtime_unit_id) = 0
            )
        )
        and (
            provider_runtime_spec_digest is null
            or provider_runtime_spec_digest ~ '^sha256:[0-9a-f]{64}$'
        )
        and (
            provider_service_port_name is null
            or (
                octet_length(provider_service_port_name) between 1 and 128
                and btrim(provider_service_port_name) <> ''
                and position(chr(13) in provider_service_port_name) = 0
                and position(chr(10) in provider_service_port_name) = 0
            )
        )
        and (
            provider_release_identity is null
            or (
                provider_release_identity ~ '^sha256:[0-9a-f]{64}$'
                and provider_release_identity = agent_artifact_digest
            )
        )
        and (
            provider_session_id is null
            or (
                octet_length(provider_session_id) between 1 and 256
                and btrim(provider_session_id) <> ''
                and provider_session_id !~ E'[\r\n]'
            )
        )
        and (
            provider_run_id is null
            or (
                octet_length(provider_run_id) between 1 and 256
                and btrim(provider_run_id) <> ''
                and provider_run_id !~ E'[\r\n]'
            )
        )
        and (provider_event_cursor is null or provider_event_cursor >= 0)
        and (
            provider_state is null
            or provider_state in (
                'created',
                'planning',
                'executing',
                'awaiting_approval',
                'verifying',
                'completed',
                'failed',
                'cancelled'
            )
        )
        and (provider_bound_at is null or provider_bound_at >= requested_at)
    ),
    add constraint agent_executions_approval_started_check check (
        status <> 'awaiting_approval' or started_at is not null
    );

alter table agent_execution_events
    drop constraint agent_execution_events_kind_check,
    add constraint agent_execution_events_kind_check check (
        kind in (
            'execution_requested',
            'model_output',
            'tool_request',
            'tool_result',
            'approval_resolved',
            'execution_failed',
            'execution_completed',
            'execution_cancelled'
        )
    );

create table agent_approval_checkpoints (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    conversation_id uuid not null,
    execution_id uuid not null,
    id uuid not null,
    provider_run_identity_digest text not null check (
        provider_run_identity_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    invocation_profile_digest text not null check (
        invocation_profile_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    source_event_sequence bigint not null check (source_event_sequence >= 0),
    call_id text not null check (
        octet_length(call_id) between 1 and 256
        and btrim(call_id) = call_id
        and call_id !~ E'[\r\n]'
    ),
    tool_name text not null check (
        octet_length(tool_name) between 1 and 128
        and tool_name ~ '^[a-z0-9]+([.-][a-z0-9]+)*$'
    ),
    tool_revision text not null check (
        octet_length(tool_revision) between 1 and 128
        and tool_revision !~ E'[\r\n]'
    ),
    tool_contract_digest text not null check (
        tool_contract_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    request_digest text not null check (request_digest ~ '^sha256:[0-9a-f]{64}$'),
    request_size_bytes bigint not null check (
        request_size_bytes between 0 and 9007199254740991
    ),
    request_media_type text not null check (
        octet_length(request_media_type) between 1 and 255
        and btrim(request_media_type) = request_media_type
        and request_media_type !~ E'[\r\n]'
    ),
    status text not null check (
        status in ('pending', 'approved', 'denied', 'expired', 'resumed', 'cancelled')
    ),
    decision_id uuid,
    outcome text check (outcome in ('approved', 'denied', 'expired')),
    decided_by uuid references identity_principals(id),
    authorization_decision_id text check (
        authorization_decision_id is null
        or (
            octet_length(authorization_decision_id) between 1 and 512
            and btrim(authorization_decision_id) = authorization_decision_id
            and authorization_decision_id !~ E'[\r\n]'
        )
    ),
    authorization_decision_digest text check (
        authorization_decision_digest is null
        or authorization_decision_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    reason text check (
        reason is null
        or (
            octet_length(reason) between 1 and 1024
            and btrim(reason) = reason
            and reason !~ E'[\r\n]'
        )
    ),
    decision_digest text check (
        decision_digest is null or decision_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    resume_command_id uuid references node_commands(id),
    resume_command_digest text check (
        resume_command_digest is null or resume_command_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    aggregate_version bigint not null check (aggregate_version > 0),
    requested_at timestamptz not null,
    expires_at timestamptz not null,
    updated_at timestamptz not null,
    decided_at timestamptz,
    resumed_at timestamptz,
    cancelled_at timestamptz,
    primary key (organization_id, id),
    unique (
        organization_id,
        execution_id,
        provider_run_identity_digest,
        source_event_sequence
    ),
    unique (organization_id, execution_id, provider_run_identity_digest, call_id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, conversation_id, execution_id)
        references agent_executions (organization_id, conversation_id, id),
    check (expires_at = requested_at + interval '1 day'),
    check (updated_at >= requested_at),
    check (decided_at is null or decided_at between requested_at and updated_at),
    check (resumed_at is null or resumed_at between requested_at and updated_at),
    check (cancelled_at is null or cancelled_at between requested_at and updated_at),
    check (
        (decision_id is not null)
        = (
            outcome is not null
            and decision_digest is not null
            and decided_at is not null
        )
    ),
    check (
        (decided_by is not null) = (authorization_decision_id is not null)
        and (decided_by is not null) = (authorization_decision_digest is not null)
    ),
    check (
        (outcome in ('approved', 'denied'))
        = (decided_by is not null)
    ),
    check (
        outcome <> 'expired'
        or (
            decided_by is null
            and reason is null
            and decided_at >= expires_at
        )
    ),
    check (
        outcome not in ('approved', 'denied')
        or decided_at < expires_at
    ),
    check (
        (resume_command_id is not null)
        = (resume_command_digest is not null and resumed_at is not null)
    ),
    check (
        (status = 'pending' and decision_id is null and resume_command_id is null and cancelled_at is null)
        or (
            status = 'approved'
            and outcome = 'approved'
            and decision_id is not null
            and resume_command_id is null
            and cancelled_at is null
        )
        or (
            status = 'denied'
            and outcome = 'denied'
            and decision_id is not null
            and resume_command_id is null
            and cancelled_at is null
        )
        or (
            status = 'expired'
            and outcome = 'expired'
            and decision_id is not null
            and resume_command_id is null
            and cancelled_at is null
        )
        or (
            status = 'resumed'
            and decision_id is not null
            and resume_command_id is not null
            and cancelled_at is null
        )
        or (
            status = 'cancelled'
            and resume_command_id is null
            and cancelled_at = updated_at
        )
    )
);

create unique index agent_approval_checkpoints_one_active_per_execution_idx
    on agent_approval_checkpoints (organization_id, execution_id)
    where status in ('pending', 'approved', 'denied', 'expired');

create unique index agent_approval_checkpoints_decision_idx
    on agent_approval_checkpoints (organization_id, decision_id)
    where decision_id is not null;

create index agent_approval_checkpoints_execution_time_idx
    on agent_approval_checkpoints (
        organization_id,
        execution_id,
        requested_at desc,
        id desc
    );

create index agent_approval_checkpoints_expiry_idx
    on agent_approval_checkpoints (expires_at, organization_id, id)
    where status = 'pending';

comment on table agent_approval_checkpoints is
    'A1.5 exact approval-required Agent Tool call checkpoints; stores immutable identity evidence only, never Tool payload or Secret material';

comment on column agent_execution_events.kind is
    'Closed Agent semantic event kind; approval decisions join the sole conversation sequence without copying Tool payload or Secret material';
