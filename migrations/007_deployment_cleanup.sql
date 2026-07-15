alter table deployments
    add column cleanup_command_id uuid,
    add column cancellation_requested_at timestamptz,
    add column cancelled_at timestamptz;

alter table deployments
    add constraint deployments_cleanup_command_fk
    foreign key (node_id, cleanup_command_id)
    references node_commands (node_id, id);

do $$
declare
    constraint_row record;
begin
    for constraint_row in
        select conname
        from pg_constraint
        where conrelid = 'deployments'::regclass
          and contype = 'c'
          and pg_get_constraintdef(oid) not like '%aggregate_version%'
    loop
        execute format(
            'alter table deployments drop constraint %I',
            constraint_row.conname
        );
    end loop;
end
$$;

alter table deployments
    add constraint deployments_status_check check (
        status in (
            'queued', 'resolving', 'scheduled', 'applying', 'verifying',
            'cancelling', 'cleanup_pending', 'active', 'failed', 'orphaned',
            'cancelled'
        )
    ),
    add constraint deployments_time_check check (updated_at >= requested_at),
    add constraint deployments_failure_check check (
        (status in ('failed', 'orphaned')) = (failure is not null)
    ),
    add constraint deployments_activation_check check (
        (status = 'active') = (activated_at is not null)
    ),
    add constraint deployments_cancellation_request_check check (
        (status in ('cancelling', 'cleanup_pending', 'orphaned', 'cancelled')) =
        (cancellation_requested_at is not null)
    ),
    add constraint deployments_cancelled_at_check check (
        (status = 'cancelled') = (cancelled_at is not null)
    ),
    add constraint deployments_node_check check (
        status not in (
            'scheduled', 'applying', 'verifying', 'cleanup_pending', 'active',
            'orphaned'
        ) or node_id is not null
    ),
    add constraint deployments_apply_command_check check (
        status not in ('applying', 'verifying', 'cleanup_pending', 'active', 'orphaned')
        or command_id is not null
    ),
    add constraint deployments_cleanup_command_check check (
        (status = 'cleanup_pending' and cleanup_command_id is not null)
        or (status in ('orphaned', 'cancelled'))
        or (status not in ('cleanup_pending', 'orphaned', 'cancelled') and cleanup_command_id is null)
    );

drop index deployments_reconcile_idx;

create index deployments_reconcile_idx
    on deployments (status, updated_at, id)
    where status not in ('active', 'failed', 'orphaned', 'cancelled');
