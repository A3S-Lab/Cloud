create table application_delivery_credentials (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    id uuid not null,
    audience text not null check (audience = 'anonymous'),
    lookup_key text not null
        check (
            char_length(lookup_key) between 1 and 64
            and lookup_key ~ '^[A-Za-z0-9]([A-Za-z0-9_-]*[A-Za-z0-9])?$'
            and position('--' in lookup_key) = 0
        ),
    secret_id uuid not null,
    secret_version bigint not null check (secret_version > 0),
    generation bigint not null
        check (generation between 1 and 9007199254740991),
    status text not null check (status in ('active', 'disabled', 'revoked')),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    revoked_at timestamptz,
    primary key (organization_id, application_id, id),
    unique (organization_id, project_id, application_id, id),
    unique (organization_id, project_id, application_id, lookup_key),
    foreign key (organization_id, project_id, application_id)
        references applications (organization_id, project_id, id),
    foreign key (secret_id, secret_version)
        references secret_versions (secret_id, version),
    check (updated_at >= created_at),
    check (
        status = 'revoked'
            and revoked_at is not null
            and revoked_at >= updated_at
        or status in ('active', 'disabled')
            and revoked_at is null
    )
);

create index application_delivery_credentials_application_idx
    on application_delivery_credentials (
        organization_id,
        project_id,
        application_id,
        created_at,
        id
    );

create index application_delivery_credentials_lookup_idx
    on application_delivery_credentials (
        organization_id,
        project_id,
        application_id,
        lookup_key
    );

comment on table application_delivery_credentials is
    'Application-scoped anonymous delivery credential bindings; stores opaque lookup keys and exact Secrets version references without plaintext';
