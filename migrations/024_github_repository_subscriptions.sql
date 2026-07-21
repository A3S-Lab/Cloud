alter table github_source_connections
    add constraint github_source_connections_organization_connection_installation_key
    unique (organization_id, id, installation_id);

create table github_repository_subscriptions (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    connection_id uuid not null,
    installation_id bigint not null check (installation_id > 0),
    repository_provider text not null,
    repository_url text not null,
    repository_identity text not null,
    branch_name text not null,
    recipe jsonb not null,
    recipe_digest text not null,
    status text not null,
    aggregate_version bigint not null,
    created_at timestamptz not null,
    deactivated_at timestamptz,
    primary key (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, connection_id, installation_id)
        references github_source_connections (organization_id, id, installation_id),
    check (repository_provider = 'github'),
    check (
        repository_url ~
        '^https://github[.]com/[a-z0-9]([a-z0-9-]{0,37}[a-z0-9])?/[a-z0-9._-]{1,100}$'
    ),
    check (
        repository_identity =
        'github:github.com/' || substring(repository_url from 20)
    ),
    check (
        char_length(branch_name) between 1 and 255
        and branch_name !~ '^refs/'
        and branch_name !~ '^/'
        and branch_name !~ '/$'
        and branch_name !~ '[.]$'
        and branch_name !~ '[.][.]'
        and branch_name !~ '//'
        and branch_name <> '@'
        and branch_name !~ '(^|/)[.]'
        and branch_name !~ '[.](/|$)'
        and branch_name !~ '[.]lock(/|$)'
        and branch_name ~ '^[A-Za-z0-9._/-]+$'
    ),
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
    check (recipe_digest ~ '^sha256:[0-9a-f]{64}$'),
    check (status in ('active', 'inactive')),
    check (
        (
            status = 'active'
            and aggregate_version = 1
            and deactivated_at is null
        )
        or (
            status = 'inactive'
            and aggregate_version = 2
            and deactivated_at is not null
            and deactivated_at >= created_at
        )
    )
);

create unique index github_repository_subscriptions_active_identity_idx
    on github_repository_subscriptions (
        organization_id,
        project_id,
        environment_id,
        connection_id,
        repository_identity,
        branch_name,
        recipe_digest
    )
    where status = 'active';

create index github_repository_subscriptions_environment_time_idx
    on github_repository_subscriptions (
        organization_id,
        project_id,
        environment_id,
        created_at,
        id
    );

create index github_repository_subscriptions_fanout_idx
    on github_repository_subscriptions (
        installation_id,
        repository_identity,
        branch_name,
        organization_id,
        id
    )
    where status = 'active';
