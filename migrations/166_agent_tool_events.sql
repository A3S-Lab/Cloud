alter table agent_execution_events
    drop constraint agent_execution_events_kind_check,
    add constraint agent_execution_events_kind_check check (
        kind in (
            'execution_requested',
            'model_output',
            'tool_request',
            'tool_result',
            'execution_failed',
            'execution_completed',
            'execution_cancelled'
        )
    );

comment on column agent_execution_events.kind is
    'Closed Agent semantic event kind; Tool request/result records carry only exact binding and payload identity evidence, never Tool payload or Secret material';
