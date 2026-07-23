alter table github_source_connections
    add column provider_checked_at timestamptz,
    add column provider_check_attempted_at timestamptz,
    add column provider_next_check_at timestamptz,
    add column provider_check_failures bigint,
    add column provider_check_error text;

update github_source_connections
set provider_checked_at = connected_at,
    provider_check_attempted_at = connected_at,
    provider_next_check_at = connected_at,
    provider_check_failures = 0;

alter table github_source_connections
    alter column provider_checked_at set not null,
    alter column provider_check_attempted_at set not null,
    alter column provider_next_check_at set not null,
    alter column provider_check_failures set not null,
    add check (provider_checked_at >= connected_at),
    add check (provider_check_attempted_at >= provider_checked_at),
    add check (provider_next_check_at >= provider_check_attempted_at),
    add check (provider_check_failures >= 0 and provider_check_failures <= 4294967295),
    add check (
        provider_check_error is null
        or provider_check_error in ('not_configured', 'unavailable', 'protocol')
    ),
    add check (
        (provider_check_failures = 0 and provider_check_error is null)
        or (provider_check_failures > 0 and provider_check_error is not null)
    );

create index github_source_connections_provider_check_idx
    on github_source_connections (provider_next_check_at, organization_id, id)
    where status in ('active', 'suspended', 'installation_deleted', 'account_changed');
