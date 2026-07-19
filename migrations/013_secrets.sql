create table secrets (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    name text not null,
    name_key text not null,
    state text not null check (state in ('active', 'revoked')),
    current_version bigint not null check (current_version > 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    revoked_at timestamptz,
    unique (organization_id, project_id, environment_id, name_key),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (char_length(name) between 1 and 63),
    check (char_length(name_key) between 1 and 63),
    check (current_version <= aggregate_version),
    check (updated_at >= created_at),
    check (
        state = 'active' and revoked_at is null
        or state = 'revoked' and revoked_at = updated_at
    )
);

create index secrets_environment_idx
    on secrets (organization_id, project_id, environment_id, created_at, id);

create table secret_versions (
    secret_id uuid not null references secrets(id),
    version bigint not null check (version > 0),
    key_id text not null,
    ciphertext text not null,
    state text not null check (state in ('active', 'revoked')),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    revoked_at timestamptz,
    primary key (secret_id, version),
    check (
        octet_length(key_id) between 1 and 512
        and key_id = btrim(key_id)
        and position(chr(10) in key_id) = 0
        and position(chr(13) in key_id) = 0
    ),
    check (
        octet_length(ciphertext) between 1 and 2097152
        and ciphertext = btrim(ciphertext)
        and position(chr(10) in ciphertext) = 0
        and position(chr(13) in ciphertext) = 0
    ),
    check (
        state = 'active' and revoked_at is null
        or state = 'revoked' and revoked_at >= created_at
    )
);

alter table secrets
    add constraint secrets_current_version_fk
    foreign key (id, current_version)
    references secret_versions (secret_id, version)
    deferrable initially deferred;
