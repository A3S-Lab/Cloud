create table mcp_credentials (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    prefix text not null
        check (prefix ~ '^a3s_mcp_[a-z0-9]{16}$'),
    verifier_hash text not null
        check (
            octet_length(verifier_hash) between 1 and 512
            and verifier_hash like '$argon2id$v=19$%'
        ),
    generation bigint not null
        check (generation between 1 and 9007199254740991),
    aggregate_version bigint not null
        check (aggregate_version between 1 and 9007199254740991),
    expires_at timestamptz not null,
    created_at timestamptz not null,
    updated_at timestamptz not null,
    revoked_at timestamptz,
    unique (organization_id, id),
    unique (prefix),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (updated_at >= created_at),
    check (expires_at > created_at),
    check (
        revoked_at is null
        or (
            revoked_at = updated_at
            and revoked_at >= created_at
        )
    )
);

create index mcp_credentials_environment_idx
    on mcp_credentials (
        organization_id,
        project_id,
        environment_id,
        created_at,
        id
    );

create index mcp_credentials_expiry_idx
    on mcp_credentials (expires_at, id)
    where revoked_at is null;

comment on table mcp_credentials is
    'Environment-scoped hosted MCP credential authority; stores only fixed lookup prefixes and redacted Argon2id verifiers';
