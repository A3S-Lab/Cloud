alter table workflow_runs
    add column cancellation_requested_by uuid;

do $$
begin
    if exists (
        select 1
        from workflow_runs as run
        where run.cancellation_requested_at is not null
          and (
              select count(distinct audit.actor_id)
              from audit_records as audit
              where audit.organization_id = run.organization_id
                and audit.aggregate_id = run.id
                and audit.action = 'workflow.run.cancellation-requested'
                and audit.occurred_at = run.cancellation_requested_at
                and audit.actor_id is not null
          ) <> 1
    ) then
        raise exception 'WorkflowRun cancellation actor cannot be recovered from exact audit evidence';
    end if;
end
$$;

update workflow_runs as run
set cancellation_requested_by = (
    select audit.actor_id
    from audit_records as audit
    where audit.organization_id = run.organization_id
      and audit.aggregate_id = run.id
      and audit.action = 'workflow.run.cancellation-requested'
      and audit.occurred_at = run.cancellation_requested_at
      and audit.actor_id is not null
    order by audit.audit_id
    limit 1
)
where run.cancellation_requested_at is not null;

alter table workflow_runs
    add constraint workflow_runs_cancellation_requested_by_fk
    foreign key (cancellation_requested_by)
    references identity_principals (id);

alter table workflow_runs
    add constraint workflow_runs_cancellation_authority_check check (
        (cancellation_requested_at is null) = (cancellation_requested_by is null)
    );

create index human_tasks_parent_cancellation_candidates_idx
    on human_tasks (organization_id, workflow_run_id, id)
    where status in ('pending_activation', 'ready', 'claimed');

alter table workflow_resume_receipts
    drop constraint workflow_resume_receipts_disposition_check;

alter table workflow_resume_receipts
    add constraint workflow_resume_receipts_disposition_check check (
        disposition in ('hook_received', 'run_timed_out', 'run_cancelled')
    );

create or replace function protect_workflow_run_authority()
returns trigger
language plpgsql
as $$
begin
    if old.organization_id is distinct from new.organization_id
        or old.project_id is distinct from new.project_id
        or old.id is distinct from new.id
        or old.workflow_goal_id is distinct from new.workflow_goal_id
        or old.plan_revision_id is distinct from new.plan_revision_id
        or old.plan_digest is distinct from new.plan_digest
        or old.operation_id is distinct from new.operation_id
        or old.flow_run_id is distinct from new.flow_run_id
        or old.execution_input is distinct from new.execution_input
        or old.execution_input_digest is distinct from new.execution_input_digest
        or old.requested_by is distinct from new.requested_by
        or old.requested_at is distinct from new.requested_at
        or (
            old.cancellation_requested_at is not null
            and (
                old.cancellation_requested_at is distinct from new.cancellation_requested_at
                or old.cancellation_requested_by is distinct from new.cancellation_requested_by
                or old.cancellation_reason is distinct from new.cancellation_reason
            )
        )
    then
        raise exception 'WorkflowRun immutable authority cannot be changed';
    end if;
    return new;
end
$$;

comment on column workflow_runs.cancellation_requested_by is
    'Exact Principal that requested the immutable WorkflowRun cancellation authority';

comment on table workflow_resume_outbox is
    'Durable exactly-bound A3S Flow hook resume intent settled by HookReceived or exact terminal timeout/cancellation evidence';

comment on table workflow_resume_receipts is
    'Immutable exact Flow evidence that a resume was received or superseded by the matching run timeout/cancellation';
