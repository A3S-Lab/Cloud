create table application_publication_route_intents (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    id uuid not null,
    channels text[] not null
        check (
            cardinality(channels) >= 1
            and channels <@ array[
                'api_blocking',
                'api_streaming',
                'embed',
                'mcp',
                'web',
                'internal'
            ]::text[]
        ),
    embed_origin_allowlist text[] not null default '{}',
    rate_shaping_profile_id text not null
        check (char_length(rate_shaping_profile_id) between 1 and 128),
    rate_shaping_policy_revision_digest text not null
        check (rate_shaping_policy_revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    primary key (organization_id, application_id, id),
    unique (organization_id, project_id, application_id, id),
    foreign key (organization_id, project_id, application_id)
        references applications (organization_id, project_id, id),
    foreign key (
        organization_id,
        project_id,
        application_id,
        application_release_id
    ) references application_releases (
        organization_id,
        project_id,
        application_id,
        id
    ),
    foreign key (
        organization_id,
        application_id,
        application_release_id,
        application_release_digest
    ) references application_releases (
        organization_id,
        application_id,
        id,
        contract_digest
    )
);

create index application_publication_route_intents_release_idx
    on application_publication_route_intents (
        organization_id,
        project_id,
        application_id,
        application_release_id,
        application_release_digest,
        id
    );

create function reject_application_publication_route_intent_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Application publication route intents are immutable';
end
$$;

create trigger application_publication_route_intents_immutable
before update or delete on application_publication_route_intents
for each row execute function reject_application_publication_route_intent_mutation();

comment on table application_publication_route_intents is
    'Immutable Applications-owned exact-release publication route intents; exact create replays by primary key without CQRS or Gateway apply';