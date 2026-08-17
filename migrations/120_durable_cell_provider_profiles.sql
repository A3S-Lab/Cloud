alter table durable_cell_deployments
    add column storage_provider_profile_acl text,
    add constraint durable_cell_deployments_provider_profile_acl_check check (
        storage_provider_profile_acl is null
        or octet_length(storage_provider_profile_acl) between 1 and 16384
    );

comment on column durable_cell_deployments.storage_provider_profile_acl is
    'Canonical non-secret A3S ACL needed to reconstruct the exact S0 publication Task; null only for correlations admitted without CELL0.5-C3b''s backwards-compatible optional profile input';
