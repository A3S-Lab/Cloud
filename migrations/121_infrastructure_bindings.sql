create table infrastructure_bindings (
    binding_name varchar(128) primary key,
    binding_schema varchar(128) not null,
    binding_digest char(71) not null,
    bound_at timestamptz not null,
    constraint infrastructure_bindings_name_check check (
        binding_name ~ '^[a-z][a-z0-9-]{0,127}$'
    ),
    constraint infrastructure_bindings_schema_check check (
        binding_schema ~ '^a3s\.cloud\.[a-z0-9.-]{1,117}\.v[1-9][0-9]*$'
    ),
    constraint infrastructure_bindings_digest_check check (
        binding_digest ~ '^sha256:[0-9a-f]{64}$'
    )
);

comment on table infrastructure_bindings is
    'Create-only deployment topology identities shared by every control-plane process using this PostgreSQL authority';

comment on column infrastructure_bindings.binding_digest is
    'Non-secret SHA-256 digest of one canonical provider or filesystem identity; replacement requires an explicit migration';
