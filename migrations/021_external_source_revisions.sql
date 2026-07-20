create table external_source_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    repository_provider text not null,
    repository_url text not null,
    repository_identity text not null,
    commit_sha text not null,
    recipe jsonb not null,
    recipe_digest text not null,
    aggregate_version bigint not null check (aggregate_version = 1),
    accepted_at timestamptz not null,
    primary key (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    unique (
        organization_id,
        project_id,
        environment_id,
        repository_identity,
        commit_sha,
        recipe_digest
    ),
    check (repository_provider = 'github'),
    check (
        repository_url ~
        '^https://github[.]com/[a-z0-9]([a-z0-9-]{0,37}[a-z0-9])?/[a-z0-9._-]{1,100}$'
    ),
    check (
        repository_identity =
        'github:github.com/' || substring(repository_url from 20)
    ),
    check (commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    check (jsonb_typeof(recipe) = 'object'),
    check (
        recipe ?& array[
            'schema',
            'kind',
            'contextPath',
            'dockerfilePath',
            'target',
            'platforms'
        ]
    ),
    check (recipe ->> 'schema' = 'a3s.cloud.build-recipe.v1'),
    check (recipe ->> 'kind' = 'dockerfile'),
    check (jsonb_typeof(recipe -> 'contextPath') = 'string'),
    check (jsonb_typeof(recipe -> 'dockerfilePath') = 'string'),
    check (
        recipe -> 'target' = 'null'::jsonb
        or jsonb_typeof(recipe -> 'target') = 'string'
    ),
    check (jsonb_typeof(recipe -> 'platforms') = 'array'),
    check (recipe_digest ~ '^sha256:[0-9a-f]{64}$')
);

create index external_source_revisions_environment_time_idx
    on external_source_revisions (
        organization_id,
        project_id,
        environment_id,
        accepted_at,
        id
    );

create table source_webhook_deliveries (
    organization_id uuid not null references organizations(id),
    provider text not null,
    delivery_id text not null,
    source_identity_digest text not null,
    received_at timestamptz not null,
    primary key (organization_id, provider, delivery_id),
    check (provider = 'github'),
    check (
        delivery_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    check (source_identity_digest ~ '^sha256:[0-9a-f]{64}$')
);
