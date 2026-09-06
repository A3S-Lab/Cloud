-- AUT0.3-C10: one durable, exact invocation-admission authority.
-- The row retains the immutable envelope and digest. It is not a timer,
-- queue, worker, target-execution, or mutable revision store.

create table automation_invocations (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    automation_id uuid not null,
    invocation_id uuid not null,
    revision_id uuid not null,
    revision_digest text not null
        check (revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    deduplication_key text not null
        check (char_length(deduplication_key) between 1 and 1024)
        check (deduplication_key !~ '[\r\n]'),
    invocation_digest text not null
        check (invocation_digest ~ '^sha256:[0-9a-f]{64}$'),
    invocation_json jsonb not null
        check (jsonb_typeof(invocation_json) = 'object'),
    requested_at timestamptz not null,
    admitted_at timestamptz not null,
    primary key (organization_id, invocation_id),
    unique (organization_id, automation_id, deduplication_key),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (invocation_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    check ((invocation_json ->> 'schema') = 'cloud.automation.invocation.v1'),
    check ((invocation_json ->> 'invocationId')::uuid = invocation_id),
    check ((invocation_json ->> 'automationId')::uuid = automation_id),
    check ((invocation_json ->> 'automationRevisionId')::uuid = revision_id),
    check ((invocation_json ->> 'automationRevisionDigest') = revision_digest),
    check ((invocation_json ->> 'organizationId')::uuid = organization_id),
    check ((invocation_json ->> 'projectId')::uuid = project_id),
    check ((invocation_json ->> 'environmentId')::uuid = environment_id),
    check ((invocation_json ->> 'deduplicationKey') = deduplication_key),
    check ((invocation_json ->> 'requestedAt')::timestamptz = requested_at),
    check (admitted_at >= requested_at)
);

create index automation_invocations_recovery_idx
    on automation_invocations (
        organization_id,
        environment_id,
        admitted_at,
        invocation_id
    );

create function enforce_automation_invocation_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Automation invocation admission cannot be deleted';
    end if;
    if old.organization_id <> new.organization_id
       or old.project_id <> new.project_id
       or old.environment_id <> new.environment_id
       or old.automation_id <> new.automation_id
       or old.invocation_id <> new.invocation_id
       or old.revision_id <> new.revision_id
       or old.revision_digest <> new.revision_digest
       or old.deduplication_key <> new.deduplication_key
       or old.invocation_digest <> new.invocation_digest
       or old.invocation_json <> new.invocation_json
       or old.requested_at <> new.requested_at
       or old.admitted_at <> new.admitted_at then
        raise exception 'Automation invocation admission is immutable';
    end if;
    return new;
end
$$;

create trigger automation_invocations_immutable
before update or delete on automation_invocations
for each row execute function enforce_automation_invocation_immutability();

comment on table automation_invocations is
    'Exact Automations invocation admission and replay authority; not a scheduler, queue, worker, or target store';

comment on column automation_invocations.invocation_digest is
    'Canonical digest of invocation_json used to fence immutable replay evidence';
