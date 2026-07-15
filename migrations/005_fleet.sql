create table enrollment_tokens (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    name text not null,
    name_key text not null,
    token_hash text not null unique,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    expires_at timestamptz not null,
    used_at timestamptz,
    revoked_at timestamptz,
    unique (organization_id, name_key),
    check (expires_at > created_at),
    check (used_at is null or used_at >= created_at),
    check (revoked_at is null or revoked_at >= created_at)
);

create table nodes (
    organization_id uuid not null references organizations(id),
    id uuid not null unique,
    name text not null,
    name_key text not null,
    state text not null check (state in ('pending', 'ready', 'draining', 'revoked')),
    agent_instance_id uuid not null,
    agent_version text not null,
    runtime_provider_id text not null,
    runtime_provider_build text not null,
    capabilities_digest text not null,
    capabilities jsonb not null,
    enrolled_at timestamptz not null,
    last_observed_at timestamptz not null,
    last_sequence bigint not null default 0 check (last_sequence >= 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    primary key (organization_id, id),
    unique (organization_id, name_key),
    check (last_observed_at >= enrolled_at)
);

create table node_enrollment_reservations (
    enrollment_token_id uuid primary key references enrollment_tokens(id),
    node_id uuid not null unique references nodes(id),
    request_digest text not null,
    reserved_at timestamptz not null
);

create table node_certificates (
    id uuid primary key,
    node_id uuid not null references nodes(id),
    serial_number text not null,
    fingerprint text not null unique,
    certificate_pem text not null,
    ca_bundle_pem text not null,
    issued_at timestamptz not null,
    expires_at timestamptz not null,
    revoked_at timestamptz,
    check (expires_at > issued_at),
    check (revoked_at is null or revoked_at >= issued_at)
);

create unique index node_certificates_one_active_idx
    on node_certificates (node_id)
    where revoked_at is null;

create index node_certificates_fingerprint_idx
    on node_certificates (fingerprint);

create table node_certificate_rotations (
    scope_key text not null,
    idempotency_key text not null,
    request_digest text not null,
    organization_id uuid not null,
    node_id uuid not null,
    current_certificate_id uuid not null references node_certificates(id),
    replacement_certificate_id uuid not null,
    requested_at timestamptz not null,
    completed_at timestamptz,
    primary key (scope_key, idempotency_key),
    foreign key (organization_id, node_id) references nodes (organization_id, id)
);

create unique index node_certificate_rotations_one_pending_idx
    on node_certificate_rotations (node_id)
    where completed_at is null;

create table node_commands (
    id uuid primary key,
    node_id uuid not null references nodes(id),
    sequence bigint not null check (sequence > 0),
    aggregate_id uuid not null,
    generation bigint not null check (generation > 0),
    command_kind text not null check (
        command_kind in ('runtime_apply', 'runtime_inspect', 'runtime_stop', 'runtime_remove')
    ),
    payload_schema text not null,
    payload_digest text not null,
    payload jsonb not null,
    issued_at timestamptz not null,
    not_after timestamptz not null,
    correlation_id uuid not null,
    lease_id uuid,
    leased_to_agent_instance_id uuid,
    leased_until timestamptz,
    acknowledgement jsonb,
    completed_at timestamptz,
    unique (node_id, sequence),
    check (not_after > issued_at),
    check (
        (lease_id is null and leased_to_agent_instance_id is null and leased_until is null)
        or
        (lease_id is not null and leased_to_agent_instance_id is not null and leased_until is not null)
    ),
    check (
        (acknowledgement is null and completed_at is null)
        or
        (acknowledgement is not null and completed_at is not null)
    )
);

create index node_commands_lease_idx
    on node_commands (node_id, sequence)
    where acknowledgement is null;

create unique index node_commands_one_apply_generation_idx
    on node_commands (node_id, aggregate_id, generation)
    where command_kind = 'runtime_apply';

create table runtime_observations (
    report_id uuid primary key,
    node_id uuid not null references nodes(id),
    command_id uuid references node_commands(id),
    agent_instance_id uuid not null,
    observed_at timestamptz not null,
    received_at timestamptz not null,
    unit_id text not null,
    generation bigint not null check (generation > 0),
    observation jsonb not null
);

create index runtime_observations_unit_idx
    on runtime_observations (unit_id, generation, observed_at desc, report_id desc);

create table node_log_batches (
    batch_id uuid primary key,
    node_id uuid not null references nodes(id),
    payload_digest text not null,
    sent_at timestamptz not null,
    received_at timestamptz not null,
    chunk_count integer not null check (chunk_count between 1 and 256)
);

create table node_log_chunks (
    node_id uuid not null references nodes(id),
    unit_id text not null,
    generation bigint not null check (generation > 0),
    cursor_value text not null check (octet_length(cursor_value) between 1 and 1024),
    sequence bigint not null,
    observed_at_ms bigint not null check (observed_at_ms >= 0),
    stream text not null check (stream in ('stdout', 'stderr')),
    checksum text not null,
    received_at timestamptz not null,
    object_key text not null unique,
    primary key (node_id, unit_id, generation, sequence),
    unique (node_id, unit_id, generation, cursor_value)
);

create table node_log_batch_chunks (
    batch_id uuid not null references node_log_batches(batch_id),
    ordinal integer not null check (ordinal between 0 and 255),
    node_id uuid not null,
    unit_id text not null,
    generation bigint not null,
    sequence bigint not null,
    primary key (batch_id, ordinal),
    unique (batch_id, node_id, unit_id, generation, sequence),
    foreign key (node_id, unit_id, generation, sequence)
        references node_log_chunks (node_id, unit_id, generation, sequence)
);

create index node_log_chunks_ordered_idx
    on node_log_chunks (node_id, unit_id, generation, sequence);

create table node_gateway_acknowledgements (
    acknowledgement_id uuid primary key,
    node_id uuid not null references nodes(id),
    revision bigint not null check (revision > 0),
    snapshot_digest text not null,
    state text not null check (state in ('applied', 'rejected')),
    message text,
    acknowledged_at timestamptz not null,
    received_at timestamptz not null,
    unique (node_id, revision, snapshot_digest)
);
