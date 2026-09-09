-- I0.2c: durable Inference usage ledger for Gateway→Cloud batch ingest.
-- Stores only watermark + event digests required for contiguous ACK,
-- wrong-after hold, and event-id conflict detection. Never stores raw
-- inference payloads, billing amounts, or credential secrets.

create table inference_usage_watermarks (
    organization_id uuid not null check (
        organization_id <> '00000000-0000-0000-0000-000000000000'
    ),
    gateway_id uuid not null check (
        gateway_id <> '00000000-0000-0000-0000-000000000000'
    ),
    boot_epoch uuid check (
        boot_epoch is null
        or boot_epoch <> '00000000-0000-0000-0000-000000000000'
    ),
    sequence bigint check (sequence is null or sequence > 0),
    updated_at timestamptz not null,
    primary key (organization_id, gateway_id),
    check ((boot_epoch is null) = (sequence is null))
);

create table inference_usage_events (
    organization_id uuid not null check (
        organization_id <> '00000000-0000-0000-0000-000000000000'
    ),
    gateway_id uuid not null check (
        gateway_id <> '00000000-0000-0000-0000-000000000000'
    ),
    event_id uuid not null check (
        event_id <> '00000000-0000-0000-0000-000000000000'
    ),
    payload_sha256 text not null check (
        payload_sha256 ~ '^[0-9a-f]{64}$'
    ),
    boot_epoch uuid not null check (
        boot_epoch <> '00000000-0000-0000-0000-000000000000'
    ),
    sequence bigint not null check (sequence > 0),
    batch_id uuid not null check (
        batch_id <> '00000000-0000-0000-0000-000000000000'
    ),
    accepted_at timestamptz not null,
    primary key (organization_id, gateway_id, event_id),
    foreign key (organization_id, gateway_id)
        references inference_usage_watermarks (organization_id, gateway_id)
);

create index inference_usage_events_gateway_cursor_idx
    on inference_usage_events (
        organization_id,
        gateway_id,
        boot_epoch,
        sequence
    );
