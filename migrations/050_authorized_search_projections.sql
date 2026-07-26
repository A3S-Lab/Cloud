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
    'Credential-free C0 resource metadata registered for tenant-authorized global search';
