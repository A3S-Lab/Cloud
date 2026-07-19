alter table node_log_chunks
    add column retained_at timestamptz;

alter table node_log_chunks
    add constraint node_log_chunks_retained_at_check
    check (retained_at is null or retained_at >= received_at);

create index node_log_chunks_retention_idx
    on node_log_chunks (received_at, node_id, unit_id, generation, sequence)
    where retained_at is null;
