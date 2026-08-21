alter table node_commands
    add constraint node_commands_writer_fence_authority_unique
    unique (node_id, id, aggregate_id, generation, command_kind, payload_digest);

create table workload_writer_fence_receipts (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    workload_revision_id uuid not null,
    workload_revision_generation bigint not null
        check (workload_revision_generation > 0),
    replica_id uuid not null,
    replica_ordinal integer not null
        check (replica_ordinal >= 0 and replica_ordinal < 100),
    writer_epoch bigint not null check (writer_epoch > 0),
    member_id uuid not null,
    placement_generation bigint not null check (placement_generation > 0),
    managed_owner_kind text not null
        check (
            length(managed_owner_kind) <= 64
            and managed_owner_kind ~ '^[a-z][a-z0-9-]{0,31}(\.[a-z][a-z0-9-]{0,31})+$'
        ),
    managed_owner_id uuid not null,
    managed_owner_generation bigint not null check (managed_owner_generation > 0),
    managed_owner_spec_digest text not null
        check (managed_owner_spec_digest ~ '^sha256:[0-9a-f]{64}$'),
    node_id uuid not null,
    runtime_unit_id text not null
        check (octet_length(runtime_unit_id) between 1 and 512),
    command_id uuid not null,
    command_kind text not null
        check (command_kind = 'runtime_remove'),
    command_payload_digest text not null
        check (command_payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    acknowledgement_digest text not null
        check (acknowledgement_digest ~ '^sha256:[0-9a-f]{64}$'),
    continuation_operation_id uuid not null unique,
    receipt_schema text not null
        check (receipt_schema = 'cloud.workload.writer-fence-receipt.v1'),
    receipt_digest text not null unique
        check (receipt_digest ~ '^sha256:[0-9a-f]{64}$'),
    fenced_at timestamptz not null,
    primary key (organization_id, workload_id, writer_epoch),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, workload_id)
        references workload_controls (organization_id, workload_id),
    foreign key (workload_id, workload_revision_id, workload_revision_generation)
        references workload_revisions (workload_id, id, generation),
    foreign key (workload_id, replica_id, writer_epoch)
        references workload_replicas (workload_id, id, generation),
    foreign key (workload_id, replica_id, member_id)
        references workload_replica_members (workload_id, replica_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (
        node_id,
        command_id,
        replica_id,
        writer_epoch,
        command_kind,
        command_payload_digest
    ) references node_commands (
        node_id,
        id,
        aggregate_id,
        generation,
        command_kind,
        payload_digest
    ),
    foreign key (organization_id, continuation_operation_id)
        references operation_requests (organization_id, operation_id)
);

create index workload_writer_fence_receipts_latest_idx
    on workload_writer_fence_receipts (
        organization_id,
        workload_id,
        writer_epoch desc
    );

create function reject_workload_writer_fence_receipt_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Workload writer-fence receipts are immutable';
end
$$;

create trigger workload_writer_fence_receipts_immutable
before update or delete on workload_writer_fence_receipts
for each row execute function reject_workload_writer_fence_receipt_mutation();

comment on table workload_writer_fence_receipts is
    'Workloads-owned exact Runtime writer-fence receipts atomically paired with the existing Operation queue; not storage lifecycle or recovery evidence authority';

comment on column workload_writer_fence_receipts.continuation_operation_id is
    'Owner-supplied continuation atomically enqueued with the Workloads receipt';
