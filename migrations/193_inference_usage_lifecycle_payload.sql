-- I0.2c: retain prompt-free lifecycle payload bytes with each accepted usage
-- event so Cloud can project InferenceUsageRecord facts and rebuildable
-- rollups without inventing data from digests alone.

alter table inference_usage_events
    add column payload bytea not null default ''::bytea;

alter table inference_usage_events
    alter column payload drop default;

alter table inference_usage_events
    add constraint inference_usage_events_payload_not_empty
    check (octet_length(payload) > 0);
