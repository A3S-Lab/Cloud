do $$
declare
    constraint_name text;
begin
    for constraint_name in
        select conname
          from pg_constraint
         where conrelid = 'connector_execution_evidence'::regclass
           and contype = 'c'
           and pg_get_constraintdef(oid) like '%outcome%accepted%retryable%rejected%'
    loop
        execute format(
            'alter table connector_execution_evidence drop constraint %I',
            constraint_name
        );
    end loop;
end
$$;

alter table connector_execution_evidence
    add constraint connector_execution_evidence_outcome_allowed_check
        check (outcome in ('accepted', 'retryable', 'rejected', 'indeterminate')),
    add constraint connector_execution_evidence_outcome_fields_check
        check (
            outcome = 'accepted'
            and response_status between 200 and 299
            and response_digest is not null
            and response_body_bytes is not null
            and retry_after_seconds is null
            or outcome = 'retryable'
            and (response_status is null or response_status not between 200 and 299)
            and response_digest is null
            and response_body_bytes is null
            or outcome = 'rejected'
            and (response_status is null or response_status not between 200 and 299)
            and response_digest is null
            and response_body_bytes is null
            and retry_after_seconds is null
            or outcome = 'indeterminate'
            and response_status is null
            and response_digest is null
            and response_body_bytes is null
            and retry_after_seconds is null
        );

create table connector_execution_attempt_resolutions (
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
    dispatch_started_at timestamptz not null,
    outcome_deadline_at timestamptz not null,
    reason text not null
        check (
            octet_length(reason) between 1 and 1024
            and reason = btrim(reason)
            and reason !~ '[[:cntrl:]]'
        ),
    resolved_by uuid not null references identity_principals(id),
    resolved_at timestamptz not null,
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
        revision_id,
        attempt_id
    ) references connector_execution_attempts (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        attempt_id
    ),
    check (outcome_deadline_at > dispatch_started_at),
    check (resolved_at >= outcome_deadline_at)
);

create index connector_execution_attempt_resolutions_environment_time_idx
    on connector_execution_attempt_resolutions (
        organization_id,
        project_id,
        environment_id,
        resolved_at desc,
        attempt_id
    );

create function validate_connector_execution_attempt_resolution()
returns trigger
language plpgsql
as $$
declare
    stored_state text;
    stored_request_digest text;
    stored_request_body_bytes bigint;
    stored_dispatch_started_at timestamptz;
    stored_outcome_deadline_at timestamptz;
begin
    select
        state,
        request_digest,
        request_body_bytes,
        dispatch_started_at,
        outcome_deadline_at
      into
        stored_state,
        stored_request_digest,
        stored_request_body_bytes,
        stored_dispatch_started_at,
        stored_outcome_deadline_at
      from connector_execution_attempts
     where organization_id = new.organization_id
       and project_id = new.project_id
       and environment_id = new.environment_id
       and profile_id = new.profile_id
       and revision_id = new.revision_id
       and attempt_id = new.attempt_id
     for update;

    if not found
       or stored_state <> 'dispatching'
       or new.request_digest <> stored_request_digest
       or new.request_body_bytes <> stored_request_body_bytes
       or new.dispatch_started_at <> stored_dispatch_started_at
       or new.outcome_deadline_at <> stored_outcome_deadline_at
       or new.resolved_at < stored_outcome_deadline_at then
        raise exception 'Connector execution attempt resolution does not match an indeterminate attempt';
    end if;
    return new;
end
$$;

create trigger connector_execution_attempt_resolutions_validate
before insert on connector_execution_attempt_resolutions
for each row execute function validate_connector_execution_attempt_resolution();

create function reject_connector_execution_attempt_resolution_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Connector execution attempt resolutions are immutable';
end
$$;

create trigger connector_execution_attempt_resolutions_immutable
before update or delete on connector_execution_attempt_resolutions
for each row execute function reject_connector_execution_attempt_resolution_mutation();

create function require_connector_execution_attempt_resolution_evidence_pair()
returns trigger
language plpgsql
as $$
declare
    paired_outcome text;
    paired_request_digest text;
    paired_request_body_bytes bigint;
    paired_started_at timestamptz;
    paired_completed_at timestamptz;
    paired_reason text;
begin
    if tg_table_name = 'connector_execution_attempt_resolutions' then
        select
            outcome,
            request_digest,
            request_body_bytes,
            started_at,
            completed_at
          into
            paired_outcome,
            paired_request_digest,
            paired_request_body_bytes,
            paired_started_at,
            paired_completed_at
          from connector_execution_evidence
         where organization_id = new.organization_id
           and project_id = new.project_id
           and environment_id = new.environment_id
           and profile_id = new.profile_id
           and revision_id = new.revision_id
           and attempt_id = new.attempt_id;

        if paired_outcome is distinct from 'indeterminate'
           or paired_request_digest is distinct from new.request_digest
           or paired_request_body_bytes is distinct from new.request_body_bytes
           or paired_started_at is distinct from new.dispatch_started_at
           or paired_completed_at is distinct from new.resolved_at then
            raise exception 'Connector execution attempt resolution requires exact indeterminate evidence';
        end if;
    elsif new.outcome = 'indeterminate' then
        select reason
          into paired_reason
          from connector_execution_attempt_resolutions
         where organization_id = new.organization_id
           and project_id = new.project_id
           and environment_id = new.environment_id
           and profile_id = new.profile_id
           and revision_id = new.revision_id
           and attempt_id = new.attempt_id
           and request_digest = new.request_digest
           and request_body_bytes = new.request_body_bytes
           and dispatch_started_at = new.started_at
           and resolved_at = new.completed_at;

        if paired_reason is null then
            raise exception 'Indeterminate Connector execution evidence requires exact resolution authority';
        end if;
    end if;
    return null;
end
$$;

create constraint trigger connector_execution_attempt_resolution_requires_evidence
after insert on connector_execution_attempt_resolutions
deferrable initially deferred
for each row execute function require_connector_execution_attempt_resolution_evidence_pair();

create constraint trigger connector_execution_indeterminate_evidence_requires_resolution
after insert on connector_execution_evidence
deferrable initially deferred
for each row execute function require_connector_execution_attempt_resolution_evidence_pair();

comment on table connector_execution_attempt_resolutions is
    'Immutable operator recovery facts that close expired dispatches as indeterminate without authorizing provider replay';

comment on column connector_execution_attempt_resolutions.reason is
    'Bounded operator reason retained for audit and API reads; never provider response, credential, endpoint, or body material';
