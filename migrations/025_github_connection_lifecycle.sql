alter table github_source_connections
    add column status text not null default 'active',
    add column updated_at timestamptz;

update github_source_connections
set updated_at = connected_at;

alter table github_source_connections
    alter column status drop default,
    alter column updated_at set not null,
    drop constraint github_source_connections_pkey,
    drop constraint github_source_connections_id_key,
    drop constraint github_source_connections_installation_id_key,
    drop constraint github_source_connections_account_kind_account_id_key,
    add primary key (id),
    add check (
        status in (
            'active',
            'suspended',
            'verification_revoked',
            'installation_deleted',
            'account_changed'
        )
    ),
    add check (updated_at >= connected_at);

create unique index github_source_connections_current_organization_idx
    on github_source_connections (organization_id)
    where status in ('active', 'suspended');

create unique index github_source_connections_current_installation_idx
    on github_source_connections (installation_id)
    where status in ('active', 'suspended');

create unique index github_source_connections_current_account_idx
    on github_source_connections (account_kind, account_id)
    where status in ('active', 'suspended');

create index github_source_connections_organization_history_idx
    on github_source_connections (
        organization_id,
        connected_at desc,
        id desc
    );

create table github_connection_lifecycle_inbox (
    provider text not null,
    delivery_id text not null,
    event_name text not null,
    action_name text not null,
    subject_kind text not null,
    subject_id bigint not null check (subject_id > 0),
    payload_digest text not null,
    received_at timestamptz not null,
    primary key (provider, delivery_id),
    check (provider = 'github'),
    check (delivery_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    check (
        (event_name = 'installation' and action_name in ('suspend', 'unsuspend', 'deleted'))
        or (event_name = 'installation_target' and action_name = 'renamed')
        or (
            event_name = 'github_app_authorization'
            and action_name = 'revoked'
        )
    ),
    check (subject_kind in ('installation', 'user')),
    check (
        (event_name = 'github_app_authorization' and subject_kind = 'user')
        or (
            event_name in ('installation', 'installation_target')
            and subject_kind = 'installation'
        )
    ),
    check (payload_digest ~ '^sha256:[0-9a-f]{64}$')
);

create index github_connection_lifecycle_subject_time_idx
    on github_connection_lifecycle_inbox (
        subject_kind,
        subject_id,
        received_at,
        delivery_id
    );
