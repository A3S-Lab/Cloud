create table ontologies (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    name text not null check (char_length(name) between 1 and 120),
    name_key text not null check (char_length(name_key) between 1 and 120),
    description text not null check (char_length(description) <= 4096),
    current_revision_id uuid not null,
    current_revision_number bigint not null check (current_revision_number > 0),
    current_revision_digest text not null
        check (current_revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, name_key),
    unique (organization_id, project_id, id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    check (current_revision_number = aggregate_version),
    check (updated_at >= created_at)
);

create table ontology_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    ontology_id uuid not null,
    id uuid not null,
    revision_number bigint not null check (revision_number > 0),
    parent_revision_id uuid,
    parent_digest text check (parent_digest ~ '^sha256:[0-9a-f]{64}$'),
    contract_schema text not null check (contract_schema = 'cloud.workflow.ontology.v1'),
    compiler_schema_version integer not null check (compiler_schema_version = 1),
    canonical_acl text not null check (octet_length(canonical_acl) between 1 and 1048576),
    content_digest text not null check (content_digest ~ '^sha256:[0-9a-f]{64}$'),
    migration_policy text not null check (migration_policy in ('initial', 'compatible', 'explicit')),
    migration_rule_id text,
    migration_digest text check (migration_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, ontology_id, id),
    unique (organization_id, ontology_id, revision_number),
    foreign key (organization_id, project_id, ontology_id)
        references ontologies (organization_id, project_id, id),
    foreign key (organization_id, ontology_id, parent_revision_id)
        references ontology_revisions (organization_id, ontology_id, id),
    check (
        (
            revision_number = 1
            and parent_revision_id is null
            and parent_digest is null
            and migration_policy = 'initial'
            and migration_rule_id is null
            and migration_digest is null
        )
        or
        (
            revision_number > 1
            and parent_revision_id is not null
            and parent_digest is not null
            and migration_policy = 'compatible'
            and migration_rule_id is null
            and migration_digest is null
        )
        or
        (
            revision_number > 1
            and parent_revision_id is not null
            and parent_digest is not null
            and migration_policy = 'explicit'
            and migration_rule_id is not null
            and char_length(migration_rule_id) between 1 and 96
            and migration_digest is not null
        )
    )
);

alter table ontologies
    add constraint ontologies_current_revision_fk
    foreign key (organization_id, id, current_revision_id)
    references ontology_revisions (organization_id, ontology_id, id)
    deferrable initially deferred;

create index ontologies_project_updated_idx
    on ontologies (organization_id, project_id, updated_at desc, id);

create index ontology_revisions_lineage_idx
    on ontology_revisions (organization_id, ontology_id, revision_number desc, id);

create function reject_ontology_revision_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Ontology revisions are immutable';
end
$$;

create trigger ontology_revisions_immutable
before update or delete on ontology_revisions
for each row execute function reject_ontology_revision_mutation();

update api_tokens
set scopes = scopes || '["ontology:write"]'::jsonb
where (scopes ? 'platform:write' or scopes ? 'project:write')
  and not scopes ? 'ontology:write';

drop view authorized_search_projections;

create view authorized_search_projections as
with registered (
    organization_id,
    project_id,
    environment_id,
    workload_id,
    resource_kind,
    resource_id,
    title,
    description,
    state,
    updated_at
) as (
    select
        project.organization_id,
        project.id,
        null::uuid,
        null::uuid,
        'project'::text,
        project.id,
        project.name,
        'Project'::text,
        null::text,
        project.created_at
    from projects as project

    union all

    select
        ontology.organization_id,
        ontology.project_id,
        null::uuid,
        null::uuid,
        'ontology'::text,
        ontology.id,
        ontology.name,
        case
            when ontology.description = '' then 'Ontology'
            else ontology.description
        end,
        'active'::text,
        ontology.updated_at
    from ontologies as ontology

    union all

    select
        environment.organization_id,
        environment.project_id,
        environment.id,
        null::uuid,
        'environment'::text,
        environment.id,
        environment.name,
        'Environment · ' || project.name,
        null::text,
        environment.created_at
    from environments as environment
    inner join projects as project
        on project.organization_id = environment.organization_id
        and project.id = environment.project_id

    union all

    select
        node.organization_id,
        null::uuid,
        null::uuid,
        null::uuid,
        'node'::text,
        node.id,
        node.name,
        'Node · ' || node.runtime_provider_id || ' · Agent ' || node.agent_version,
        node.state,
        node.last_observed_at
    from nodes as node

    union all

    select
        workload.organization_id,
        workload.project_id,
        workload.environment_id,
        workload.id,
        'workload'::text,
        workload.id,
        workload.name,
        'Workload · desired ' || workload.desired_state,
        workload.desired_state,
        workload.updated_at
    from workloads as workload

    union all

    select
        deployment.organization_id,
        workload.project_id,
        workload.environment_id,
        deployment.workload_id,
        'deployment'::text,
        deployment.id,
        'Deployment ' || left(deployment.id::text, 8),
        'Deployment · ' || workload.name,
        deployment.status,
        deployment.updated_at
    from deployments as deployment
    inner join workloads as workload
        on workload.organization_id = deployment.organization_id
        and workload.id = deployment.workload_id

    union all

    select
        route.organization_id,
        route.project_id,
        route.environment_id,
        route.workload_id,
        'route'::text,
        route.id,
        route.hostname || route.path_prefix,
        'Route · ' || workload.name,
        route.state,
        route.updated_at
    from routes as route
    inner join workloads as workload
        on workload.organization_id = route.organization_id
        and workload.id = route.workload_id

    union all

    select
        claim.organization_id,
        claim.project_id,
        claim.environment_id,
        null::uuid,
        'domain_claim'::text,
        claim.id,
        claim.pattern,
        'Domain claim'::text,
        claim.state,
        claim.updated_at
    from domain_claims as claim

    union all

    select
        scope.organization_id,
        scope.project_id,
        scope.environment_id,
        null::uuid,
        'gateway_scope'::text,
        scope.id,
        'Gateway scope ' || left(scope.id::text, 8),
        'Gateway scope · ' || node.name,
        null::text,
        scope.updated_at
    from gateway_route_scopes as scope
    inner join nodes as node
        on node.organization_id = scope.organization_id
        and node.id = scope.node_id

    union all

    select
        build.organization_id,
        build.project_id,
        build.environment_id,
        null::uuid,
        'build_run'::text,
        build.id,
        'Build ' || left(build.id::text, 8),
        source.repository_url || ' @ ' || left(source.commit_sha, 12),
        build.status,
        build.updated_at
    from build_runs as build
    inner join external_source_revisions as source
        on source.organization_id = build.organization_id
        and source.id = build.source_revision_id

    union all

    select
        source.organization_id,
        source.project_id,
        source.environment_id,
        null::uuid,
        'source_revision'::text,
        source.id,
        source.repository_url,
        'Source revision · ' || left(source.commit_sha, 12),
        'pinned'::text,
        source.accepted_at
    from external_source_revisions as source

    union all

    select
        secret.organization_id,
        secret.project_id,
        secret.environment_id,
        null::uuid,
        'secret'::text,
        secret.id,
        secret.name,
        'Secret metadata'::text,
        secret.state,
        secret.updated_at
    from secrets as secret

    union all

    select
        request.organization_id,
        null::uuid,
        null::uuid,
        null::uuid,
        'operation'::text,
        request.operation_id,
        request.workflow_name,
        request.subject_kind || ' · ' || request.subject_id::text,
        coalesce(projection.status, 'queued'),
        coalesce(projection.updated_at, request.requested_at)
    from operation_requests as request
    left join operation_projections as projection
        on projection.operation_id = request.operation_id
)
select
    organization_id,
    project_id,
    environment_id,
    workload_id,
    resource_kind,
    resource_id,
    title,
    description,
    state,
    updated_at,
    resource_id::text as resource_id_text,
    lower(title) as title_key,
    lower(
        concat_ws(
            ' ',
            resource_kind,
            resource_id::text,
            title,
            description,
            state
        )
    ) as search_text
from registered;

comment on view authorized_search_projections is
    'Credential-free authoritative Cloud metadata, including Ontologies, for tenant-authorized global search';
