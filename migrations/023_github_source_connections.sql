create table github_connection_flows (
    id uuid primary key,
    organization_id uuid not null unique references organizations(id),
    stage text not null,
    state_digest text not null unique,
    installation_id bigint,
    pkce_verifier_digest text,
    created_at timestamptz not null,
    expires_at timestamptz not null,
    consumed_at timestamptz,
    check (stage in ('awaiting_installation', 'awaiting_oauth', 'completed')),
    check (state_digest ~ '^sha256:[0-9a-f]{64}$'),
    check (
        pkce_verifier_digest is null
        or pkce_verifier_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    check (installation_id is null or installation_id > 0),
    check (
        expires_at >= created_at + interval '1 minute'
        and expires_at <= created_at + interval '30 minutes'
    ),
    check (
        consumed_at is null
        or (consumed_at >= created_at and consumed_at < expires_at)
    ),
    check (
        (
            stage = 'awaiting_installation'
            and installation_id is null
            and pkce_verifier_digest is null
            and consumed_at is null
        )
        or (
            stage = 'awaiting_oauth'
            and installation_id is not null
            and pkce_verifier_digest is not null
            and consumed_at is null
        )
        or (
            stage = 'completed'
            and installation_id is not null
            and pkce_verifier_digest is not null
            and consumed_at is not null
        )
    )
);

create index github_connection_flows_expiry_idx
    on github_connection_flows (expires_at, organization_id);

create table github_source_connections (
    organization_id uuid primary key references organizations(id),
    id uuid not null unique,
    installation_id bigint not null unique check (installation_id > 0),
    account_id bigint not null check (account_id > 0),
    account_login text not null,
    account_kind text not null check (account_kind in ('organization', 'user')),
    verified_by_user_id bigint not null check (verified_by_user_id > 0),
    verified_by_user_login text not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    connected_at timestamptz not null,
    unique (account_kind, account_id),
    check (
        account_login ~ '^[A-Za-z0-9]([A-Za-z0-9-]{0,98}[A-Za-z0-9])?$'
    ),
    check (
        verified_by_user_login ~
        '^[A-Za-z0-9]([A-Za-z0-9-]{0,98}[A-Za-z0-9])?$'
    )
);
