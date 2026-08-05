alter table agent_executions
    add column cancellation_requested_at timestamptz,
    drop constraint agent_executions_status_check,
    add constraint agent_executions_status_check check (
        status in (
            'pending',
            'running',
            'cancelling',
            'succeeded',
            'failed',
            'cancelled'
        )
    ),
    add constraint agent_executions_cancellation_time_check check (
        cancellation_requested_at is null
        or (
            cancellation_requested_at >= requested_at
            and cancellation_requested_at <= updated_at
        )
    ),
    add constraint agent_executions_cancellation_state_check check (
        (status = 'cancelling' and cancellation_requested_at is not null)
        or (
            status in ('pending', 'running')
            and cancellation_requested_at is null
        )
        or status in ('succeeded', 'failed', 'cancelled')
    );

comment on column agent_executions.cancellation_requested_at is
    'Caller cancellation intent transported by the existing Agent Operation and A3S Code command path';
