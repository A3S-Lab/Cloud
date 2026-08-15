create table connector_execution_attempts (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    profile_id uuid not null,
    revision_id uuid not null,
    attempt_id uuid not null,
    request_digest text not null
        check (request_digest ~ '^sha256:[0-9a-f]{64}$'),
    request_body_bytes bigint not null
        check (request_body_bytes between 0 and 1048576),
    state text not null
        check (state in ('reserved', 'dispatching', 'terminal')),
    fence_generation bigint not null
        check (fence_generation > 0),
    fence_token uuid not null,
    reserved_at timestamptz not null,
    lease_expires_at timestamptz not null,
    dispatch_started_at timestamptz,
    outcome_deadline_at timestamptz,
    terminal_at timestamptz,
    created_at timestamptz not null,
    primary key (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        attempt_id
    ),
    foreign key (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id
    ) references connector_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        id
    ),
    check (created_at <= reserved_at),
    check (
        lease_expires_at > reserved_at
        and lease_expires_at <= reserved_at + interval '30 seconds'
    ),
    check (
        state = 'reserved'
        and dispatch_started_at is null
        and outcome_deadline_at is null
        and terminal_at is null
        or state = 'dispatching'
        and dispatch_started_at >= reserved_at
        and dispatch_started_at < lease_expires_at
        and outcome_deadline_at > dispatch_started_at
        and outcome_deadline_at <= dispatch_started_at + interval '120 seconds'
        and terminal_at is null
        or state = 'terminal'
        and terminal_at is not null
        and (
            dispatch_started_at is null
            and outcome_deadline_at is null
            and terminal_at between reserved_at and lease_expires_at
            or dispatch_started_at >= reserved_at
            and dispatch_started_at < lease_expires_at
            and outcome_deadline_at > dispatch_started_at
            and outcome_deadline_at <= dispatch_started_at + interval '120 seconds'
            and terminal_at >= dispatch_started_at
        )
    )
);

-- C5 was component-only, but preserve any already recorded exact evidence as a
-- closed terminal attempt before making the attempt/evidence relationship mandatory.
insert into connector_execution_attempts (
    organization_id,
    project_id,
    environment_id,
    profile_id,
    revision_id,
    attempt_id,
    request_digest,
    request_body_bytes,
    state,
    fence_generation,
    fence_token,
    reserved_at,
    lease_expires_at,
    dispatch_started_at,
    outcome_deadline_at,
    terminal_at,
    created_at
)
select
    organization_id,
    project_id,
    environment_id,
    profile_id,
    revision_id,
    attempt_id,
    request_digest,
    request_body_bytes,
    'terminal',
    1,
    attempt_id,
    started_at,
    started_at + interval '30 seconds',
    started_at,
    started_at + interval '120 seconds',
    completed_at,
    started_at
from connector_execution_evidence;

alter table connector_execution_evidence
    add constraint connector_execution_evidence_attempt_fk
    foreign key (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        attempt_id
    ) references connector_execution_attempts (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        attempt_id
    );

create index connector_execution_attempts_unresolved_idx
    on connector_execution_attempts (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        created_at desc,
        attempt_id desc
    )
    where state <> 'terminal';

create index connector_execution_attempts_dispatch_recovery_idx
    on connector_execution_attempts (
        outcome_deadline_at,
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        attempt_id
    )
    where state = 'dispatching';

create function enforce_connector_execution_attempt_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Connector execution attempts cannot be deleted';
    end if;

    if old.organization_id <> new.organization_id
        or old.project_id <> new.project_id
        or old.environment_id <> new.environment_id
        or old.profile_id <> new.profile_id
        or old.revision_id <> new.revision_id
        or old.attempt_id <> new.attempt_id
        or old.request_digest <> new.request_digest
        or old.request_body_bytes <> new.request_body_bytes
        or old.created_at <> new.created_at then
        raise exception 'Connector execution attempt identity and request binding are immutable';
    end if;

    if old.state = 'terminal' then
        raise exception 'Terminal Connector execution attempts are immutable';
    end if;

    if old.state = 'reserved' and new.state = 'reserved' then
        if new.fence_generation <> old.fence_generation + 1
            or new.fence_token = old.fence_token
            or new.reserved_at < old.lease_expires_at then
            raise exception 'Connector execution reservation takeover is not fenced';
        end if;
        return new;
    end if;

    if old.state = 'reserved' and new.state in ('dispatching', 'terminal') then
        if new.fence_generation <> old.fence_generation
            or new.fence_token <> old.fence_token
            or new.reserved_at <> old.reserved_at
            or new.lease_expires_at <> old.lease_expires_at then
            raise exception 'Connector execution transition uses a stale fence';
        end if;
        return new;
    end if;

    if old.state = 'dispatching' and new.state = 'terminal' then
        if new.fence_generation <> old.fence_generation
            or new.fence_token <> old.fence_token
            or new.reserved_at <> old.reserved_at
            or new.lease_expires_at <> old.lease_expires_at
            or new.dispatch_started_at <> old.dispatch_started_at
            or new.outcome_deadline_at <> old.outcome_deadline_at then
            raise exception 'Connector execution settlement uses a stale fence';
        end if;
        return new;
    end if;

    raise exception 'Connector execution attempt transition is invalid';
end
$$;

create trigger connector_execution_attempt_transition
before update or delete on connector_execution_attempts
for each row execute function enforce_connector_execution_attempt_transition();

create function require_connector_execution_attempt_evidence_pair()
returns trigger
language plpgsql
as $$
declare
    paired_state text;
    paired_request_digest text;
    paired_request_body_bytes bigint;
    paired_terminal_at timestamptz;
    paired_outcome text;
    paired_response_status integer;
    paired_started_at timestamptz;
    paired_completed_at timestamptz;
    paired_reserved_at timestamptz;
    paired_lease_expires_at timestamptz;
    paired_dispatch_started_at timestamptz;
begin
    if tg_table_name = 'connector_execution_attempts' then
        select outcome, response_status, started_at, completed_at
        into paired_outcome, paired_response_status, paired_started_at, paired_completed_at
        from connector_execution_evidence
        where organization_id = new.organization_id
          and project_id = new.project_id
          and environment_id = new.environment_id
          and profile_id = new.profile_id
          and revision_id = new.revision_id
          and attempt_id = new.attempt_id;

        if new.state = 'terminal' then
            if paired_outcome is null
                or paired_completed_at is distinct from new.terminal_at
                or paired_started_at is distinct from coalesce(
                    new.dispatch_started_at,
                    new.reserved_at
                )
                or new.dispatch_started_at is null
                and (
                    paired_outcome = 'accepted'
                    or paired_response_status is not null
                    or new.terminal_at > new.lease_expires_at
                ) then
                raise exception 'Terminal Connector execution attempt requires exact evidence';
            end if;
        elsif paired_outcome is not null then
            raise exception 'Non-terminal Connector execution attempt cannot have evidence';
        end if;
    else
        select
            state,
            request_digest,
            request_body_bytes,
            terminal_at,
            reserved_at,
            lease_expires_at,
            dispatch_started_at
        into
            paired_state,
            paired_request_digest,
            paired_request_body_bytes,
            paired_terminal_at,
            paired_reserved_at,
            paired_lease_expires_at,
            paired_dispatch_started_at
        from connector_execution_attempts
        where organization_id = new.organization_id
          and project_id = new.project_id
          and environment_id = new.environment_id
          and profile_id = new.profile_id
          and revision_id = new.revision_id
          and attempt_id = new.attempt_id;

        if paired_state is distinct from 'terminal'
            or paired_request_digest is distinct from new.request_digest
            or paired_request_body_bytes is distinct from new.request_body_bytes
            or paired_terminal_at is distinct from new.completed_at
            or new.started_at is distinct from coalesce(
                paired_dispatch_started_at,
                paired_reserved_at
            )
            or paired_dispatch_started_at is null
            and (
                new.outcome = 'accepted'
                or new.response_status is not null
                or new.completed_at > paired_lease_expires_at
            ) then
            raise exception 'Connector execution evidence requires its exact terminal attempt';
        end if;
    end if;
    return null;
end
$$;

create constraint trigger connector_execution_attempt_requires_evidence
after insert or update on connector_execution_attempts
deferrable initially deferred
for each row execute function require_connector_execution_attempt_evidence_pair();

create constraint trigger connector_execution_evidence_requires_attempt
after insert on connector_execution_evidence
deferrable initially deferred
for each row execute function require_connector_execution_attempt_evidence_pair();

comment on table connector_execution_attempts is
    'Connectors-owned exact-attempt pre-dispatch fencing and conservative outcome recovery; not a queue, retry schedule, Flow history, provider receipt store, or acknowledgement authority';

comment on column connector_execution_attempts.state is
    'Reserved may be fenced after lease expiry; dispatching is never reacquired and becomes an indeterminate observation after its outcome deadline; terminal requires immutable evidence';

comment on column connector_execution_attempts.fence_token is
    'Opaque pre-dispatch ownership token; never a provider idempotency key, credential, retry identity, or externally visible receipt';
