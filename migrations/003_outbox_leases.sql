alter table outbox_events
    add column lease_owner uuid,
    add column leased_until timestamptz,
    add column next_attempt_at timestamptz not null default now();

alter table outbox_events
    add constraint outbox_lease_pair check (
        (lease_owner is null and leased_until is null)
        or (lease_owner is not null and leased_until is not null)
    );

drop index outbox_events_pending_idx;

create index outbox_events_pending_idx
    on outbox_events (next_attempt_at, occurred_at, event_id)
    where published_at is null;
