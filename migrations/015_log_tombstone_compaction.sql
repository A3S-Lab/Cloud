create table node_log_compaction_ranges (
    id uuid primary key,
    node_id uuid not null references nodes(id),
    unit_id text not null check (octet_length(unit_id) between 1 and 512),
    generation bigint not null check (generation > 0),
    first_sequence bigint not null check (first_sequence >= 0),
    through_sequence bigint not null check (through_sequence >= first_sequence),
    compacted_at timestamptz not null,
    unique (node_id, unit_id, generation, first_sequence)
);

create index node_log_compaction_ranges_ordered_idx
    on node_log_compaction_ranges (
        node_id,
        unit_id,
        generation,
        first_sequence,
        through_sequence
    );

create index node_log_chunks_compaction_idx
    on node_log_chunks (
        retained_at,
        node_id,
        unit_id,
        generation,
        sequence
    )
    where retained_at is not null;
