alter table mcp_credentials
    add constraint mcp_credentials_scoped_identity_unique
        unique (organization_id, project_id, environment_id, id);

create table mcp_credential_deliveries (
    credential_id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    generation bigint not null
        check (generation between 1 and 9007199254740991),
    key_id text not null
        check (
            octet_length(key_id) between 1 and 512
            and key_id = btrim(key_id)
            and position(chr(10) in key_id) = 0
            and position(chr(13) in key_id) = 0
        ),
    ciphertext text not null
        check (
            octet_length(ciphertext) between 1 and 2097152
            and ciphertext = btrim(ciphertext)
            and position(chr(10) in ciphertext) = 0
            and position(chr(13) in ciphertext) = 0
        ),
    created_at timestamptz not null,
    expires_at timestamptz not null,
    foreign key (
        organization_id,
        project_id,
        environment_id,
        credential_id
    ) references mcp_credentials (
        organization_id,
        project_id,
        environment_id,
        id
    ) on delete cascade,
    check (
        expires_at > created_at
        and expires_at <= created_at + interval '1 hour'
    )
);

create index mcp_credential_deliveries_expiry_idx
    on mcp_credential_deliveries (expires_at, credential_id);

comment on table mcp_credential_deliveries is
    'Current authenticated ciphertext for short-lived recovery of one exact hosted MCP credential generation; plaintext is never persisted';
