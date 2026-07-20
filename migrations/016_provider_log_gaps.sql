alter table node_log_batches
    drop constraint node_log_batches_chunk_count_check;

alter table node_log_batches
    add column gap_count integer not null default 0;

alter table node_log_batches
    add constraint node_log_batches_record_counts_check
    check (
        chunk_count between 0 and 256
        and gap_count between 0 and 256
        and chunk_count + gap_count between 1 and 256
    );

create table node_log_gaps (
    node_id uuid not null references nodes(id),
    unit_id text not null check (octet_length(unit_id) between 1 and 512),
    generation bigint not null check (generation > 0),
    cursor_value text check (
        cursor_value is null
        or octet_length(cursor_value) between 1 and 1024
    ),
    sequence bigint not null,
    observed_at_ms bigint not null check (observed_at_ms >= 0),
    reason text not null check (
        reason in ('cursor_lost', 'source_disconnected')
    ),
    received_at timestamptz not null,
    check (reason <> 'cursor_lost' or cursor_value is not null),
    primary key (node_id, unit_id, generation, sequence)
);

create index node_log_gaps_ordered_idx
    on node_log_gaps (node_id, unit_id, generation, sequence);

create table node_log_batch_gaps (
    batch_id uuid not null references node_log_batches(batch_id),
    ordinal integer not null check (ordinal between 0 and 255),
    node_id uuid not null,
    unit_id text not null,
    generation bigint not null,
    sequence bigint not null,
    primary key (batch_id, ordinal),
    unique (batch_id, node_id, unit_id, generation, sequence),
    foreign key (node_id, unit_id, generation, sequence)
        references node_log_gaps (node_id, unit_id, generation, sequence)
);
