create table api_tokens (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    name text not null,
    name_key text not null,
    token_hash text not null unique,
    scopes jsonb not null check (jsonb_typeof(scopes) = 'array'),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    expires_at timestamptz,
    revoked_at timestamptz,
    unique (organization_id, name_key),
    check (expires_at is null or expires_at > created_at),
    check (revoked_at is null or revoked_at >= created_at)
);

create index api_tokens_organization_idx
    on api_tokens (organization_id, created_at, id);

create index api_tokens_active_idx
    on api_tokens (token_hash)
    where revoked_at is null;
