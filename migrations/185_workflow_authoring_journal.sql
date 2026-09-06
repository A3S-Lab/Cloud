-- CFLOW-0A: tenant-scoped hosted Workflow authoring journal.
--
-- The journal stores product authoring intent and a materialized snapshot. It
-- is not a copy of Flow execution history, task queues, retries, or scheduler
-- state. Operation and snapshot bytes remain opaque to Cloud; the application
-- boundary delegates their validation/application to A3S Flow.

create table workflow_authoring_journals (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_definition_id uuid not null,
    initial_snapshot_bytes bytea not null
        check (octet_length(initial_snapshot_bytes) between 1 and 8388608),
    initial_snapshot_digest text not null
        check (initial_snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    current_snapshot_bytes bytea not null
        check (octet_length(current_snapshot_bytes) between 1 and 8388608),
    current_snapshot_digest text not null
        check (current_snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    next_sequence bigint not null
        check (next_sequence between 1 and 9223372036854775807),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, project_id, workflow_definition_id),
    foreign key (organization_id, project_id, workflow_definition_id)
        references workflow_definitions (organization_id, project_id, id),
    check (created_at = date_trunc('microseconds', created_at)),
    check (updated_at = date_trunc('microseconds', updated_at)),
    check (updated_at >= created_at)
);

create table workflow_authoring_entries (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_definition_id uuid not null,
    sequence bigint not null check (sequence between 1 and 9223372036854775807),
    operation_id text not null
        check (
            octet_length(operation_id) between 1 and 255
            and operation_id !~ E'[\\r\\n]'
        ),
    operation_digest text not null
        check (operation_digest ~ '^sha256:[0-9a-f]{64}$'),
    base_snapshot_digest text not null
        check (base_snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    result_snapshot_digest text not null
        check (result_snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    operation_bytes bytea not null
        check (octet_length(operation_bytes) between 1 and 1048576),
    primary key (organization_id, project_id, workflow_definition_id, sequence),
    unique (organization_id, project_id, workflow_definition_id, operation_id),
    foreign key (organization_id, project_id, workflow_definition_id)
        references workflow_authoring_journals (
            organization_id,
            project_id,
            workflow_definition_id
        )
);

create index workflow_authoring_entries_cursor_idx
    on workflow_authoring_entries (
        organization_id,
        project_id,
        workflow_definition_id,
        sequence
    );

create function enforce_workflow_authoring_journal_transition()
returns trigger
language plpgsql
as $$
begin
    if old.organization_id <> new.organization_id
       or old.project_id <> new.project_id
       or old.workflow_definition_id <> new.workflow_definition_id
       or old.initial_snapshot_bytes <> new.initial_snapshot_bytes
       or old.initial_snapshot_digest <> new.initial_snapshot_digest
       or old.created_at <> new.created_at then
        raise exception 'Workflow authoring journal identity and initial snapshot are immutable';
    end if;
    if new.next_sequence <> old.next_sequence + 1 then
        raise exception 'Workflow authoring journal sequence must advance by one';
    end if;
    if new.updated_at < old.updated_at then
        raise exception 'Workflow authoring journal update time cannot move backwards';
    end if;
    return new;
end
$$;

create trigger workflow_authoring_journal_transition
before update on workflow_authoring_journals
for each row execute function enforce_workflow_authoring_journal_transition();

create function reject_workflow_authoring_entry_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Workflow authoring journal entries are append-only';
end
$$;

create trigger workflow_authoring_entries_immutable
before update or delete on workflow_authoring_entries
for each row execute function reject_workflow_authoring_entry_mutation();

comment on table workflow_authoring_journals is
    'Cloud-owned tenant authoring snapshot and sequence head; not Flow execution history';

comment on column workflow_authoring_journals.initial_snapshot_bytes is
    'Opaque Flow-produced portable workflow snapshot; Cloud does not parse it';

comment on column workflow_authoring_journals.current_snapshot_bytes is
    'Opaque Flow-produced materialized snapshot at the journal head';

comment on table workflow_authoring_entries is
    'Cloud-owned ordered opaque authoring operations with operation-id idempotency';

