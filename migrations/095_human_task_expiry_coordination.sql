create index human_tasks_expiry_candidates_idx
    on human_tasks (expires_at, organization_id, id)
    where expires_at is not null
      and status in ('pending_activation', 'ready', 'claimed');

alter table workflow_resume_receipts
    rename column hook_event_sequence to flow_event_sequence;

alter table workflow_resume_receipts
    rename column hook_event_id to flow_event_id;

alter table workflow_resume_receipts
    rename column hook_received_at to flow_event_at;

alter table workflow_resume_receipts
    add column disposition text not null default 'hook_received' check (
        disposition in ('hook_received', 'run_timed_out')
    );

alter table workflow_resume_receipts
    alter column disposition drop default;

comment on table workflow_resume_outbox is
    'Durable exactly-bound A3S Flow hook resume intent settled by HookReceived or an exact terminal supersession receipt';

comment on table workflow_resume_receipts is
    'Immutable exact Flow evidence that a resume was received or superseded by the matching run deadline';
