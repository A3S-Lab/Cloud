create table inference_routes (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    router text not null
        check (octet_length(router) between 1 and 128),
    policy_revision bigint not null
        check (policy_revision between 1 and 9007199254740991),
    models jsonb not null
        check (jsonb_typeof(models) = 'array'),
    grants jsonb not null
        check (jsonb_typeof(grants) = 'array'),
    domain_claim_id uuid not null,
    gateway_scope_id uuid not null,
    hostname text not null
        check (octet_length(hostname) between 1 and 253),
    path_prefix text not null
        check (octet_length(path_prefix) between 1 and 2048),
    binding_generation bigint not null
        check (binding_generation between 1 and 9007199254740991),
    aggregate_version bigint not null
        check (aggregate_version between 1 and 9007199254740991),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    retired_at timestamptz,
    unique (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (updated_at >= created_at),
    check (
        retired_at is null
        or (
            retired_at = updated_at
            and retired_at >= created_at
        )
    )
);

create index inference_routes_environment_active_idx
    on inference_routes (
        organization_id,
        project_id,
        environment_id,
        id
    )
    where retired_at is null;

create index inference_routes_environment_idx
    on inference_routes (
        organization_id,
        project_id,
        environment_id,
        created_at,
        id
    );

comment on table inference_routes is
    'Environment-scoped Inference route catalog heads; stores immutable access-policy revisions and EdgeRouteBindingRef fields for Gateway ACL projection without inventing Edge facts';
