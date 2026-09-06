-- AUT0.3-C6: durable schedule cursor and lease fence state.
-- The row binds one exact Automation revision but does not copy its definition,
-- target, policy, invocation, scheduler, or worker authority.

create table automation_schedule_states (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    automation_id uuid not null,
    revision_id uuid not null,
    revision_digest text not null
        check (revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    cursor_at timestamptz not null,
    lease_generation bigint not null check (lease_generation >= 0),
    lease_owner_id uuid,
    lease_id uuid,
    reserved_at timestamptz,
    lease_expires_at timestamptz,
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, project_id, environment_id, automation_id),
    unique (automation_id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (automation_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    check (revision_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    check (cursor_at = date_trunc('microseconds', cursor_at)),
    check (created_at = date_trunc('microseconds', created_at)),
    check (updated_at = date_trunc('microseconds', updated_at)),
    check (updated_at >= created_at),
    check (
        lease_owner_id is null
        and lease_id is null
        and reserved_at is null
        and lease_expires_at is null
        or lease_owner_id is not null
        and lease_id is not null
        and reserved_at is not null
        and lease_expires_at is not null
        and lease_owner_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and lease_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and reserved_at = date_trunc('microseconds', reserved_at)
        and lease_expires_at = date_trunc('microseconds', lease_expires_at)
        and lease_expires_at > reserved_at
        and lease_expires_at <= reserved_at + interval '5 minutes'
    )
);

create index automation_schedule_states_recovery_idx
    on automation_schedule_states (
        environment_id,
        updated_at,
        automation_id
    );

create function enforce_automation_schedule_state_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Automation schedule state cannot be deleted';
    end if;

    if old.organization_id <> new.organization_id
       or old.project_id <> new.project_id
       or old.environment_id <> new.environment_id
       or old.automation_id <> new.automation_id
       or old.revision_id <> new.revision_id
       or old.revision_digest <> new.revision_digest
       or old.created_at <> new.created_at then
        raise exception 'Automation schedule state identity and revision binding are immutable';
    end if;

    if new.lease_generation < old.lease_generation then
        raise exception 'Automation schedule lease generation cannot move backwards';
    end if;

    if old.lease_owner_id is null and new.lease_owner_id is not null then
        if new.lease_generation <> old.lease_generation + 1
           or new.cursor_at <> old.cursor_at
           or new.reserved_at < old.updated_at then
            raise exception 'Automation schedule lease acquisition is not fenced';
        end if;
    elsif old.lease_owner_id is not null and new.lease_owner_id is null then
        if new.lease_generation <> old.lease_generation
           or new.cursor_at <= old.cursor_at
           or new.updated_at < old.reserved_at then
            raise exception 'Automation schedule cursor commit is not fenced';
        end if;
    elsif old.lease_owner_id is not null and new.lease_owner_id is not null then
        if new.lease_generation <> old.lease_generation + 1
           or new.lease_id = old.lease_id
           or new.reserved_at < old.lease_expires_at
           or new.cursor_at <> old.cursor_at then
            raise exception 'Automation schedule lease takeover is not fenced';
        end if;
    else
        if new.cursor_at <> old.cursor_at
           or new.lease_generation <> old.lease_generation then
            raise exception 'Automation schedule state transition is invalid';
        end if;
    end if;
    return new;
end
$$;

create trigger automation_schedule_state_transition
before update or delete on automation_schedule_states
for each row execute function enforce_automation_schedule_state_transition();

comment on table automation_schedule_states is
    'Automations-owned durable cursor and lease fence bound to one exact revision; not a scheduler, queue, invocation, or worker store';

comment on column automation_schedule_states.revision_digest is
    'Exact immutable Automation revision binding; the schedule definition remains owned by the revision authority';

comment on column automation_schedule_states.lease_id is
    'Opaque evaluator fence token; never a provider idempotency key or invocation identity';
