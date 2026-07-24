create table gateway_route_scopes (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    node_id uuid not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, project_id, environment_id, node_id),
    unique (id, organization_id, project_id, environment_id, node_id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (updated_at >= created_at)
);

create index gateway_route_scopes_environment_idx
    on gateway_route_scopes (
        organization_id,
        project_id,
        environment_id,
        created_at,
        id
    );

-- A legacy physical node serving multiple environments is split into one
-- deterministic logical scope per environment/node binding. The physical
-- Gateway snapshot stream remains node-addressed.
insert into gateway_route_scopes (
    id,
    organization_id,
    project_id,
    environment_id,
    node_id,
    aggregate_version,
    created_at,
    updated_at
)
select
    md5(
        route.organization_id::text || ':'
            || route.project_id::text || ':'
            || route.environment_id::text || ':'
            || route.gateway_node_id::text
    )::uuid,
    route.organization_id,
    route.project_id,
    route.environment_id,
    route.gateway_node_id,
    1,
    min(route.created_at),
    max(route.updated_at)
from routes as route
group by
    route.organization_id,
    route.project_id,
    route.environment_id,
    route.gateway_node_id;

alter table routes
    add column gateway_scope_id uuid;

update routes as route
set gateway_scope_id = scope.id
from gateway_route_scopes as scope
where scope.organization_id = route.organization_id
    and scope.project_id = route.project_id
    and scope.environment_id = route.environment_id
    and scope.node_id = route.gateway_node_id;

alter table routes
    alter column gateway_scope_id set not null,
    add constraint routes_gateway_scope_binding_fk
        foreign key (
            gateway_scope_id,
            organization_id,
            project_id,
            environment_id,
            gateway_node_id
        )
        references gateway_route_scopes (
            id,
            organization_id,
            project_id,
            environment_id,
            node_id
        );

create index routes_gateway_scope_state_idx
    on routes (gateway_scope_id, state, hostname, path_prefix, id);

update gateway_route_cutovers as cutover
set routes = (
    select jsonb_agg(
        document
            || jsonb_build_object(
                'gateway_scope_id',
                authoritative.gateway_scope_id
            )
        order by ordinality
    )
    from jsonb_array_elements(cutover.routes)
        with ordinality as candidate(document, ordinality)
    join routes as authoritative
        on authoritative.id::text = candidate.document ->> 'id'
);

update idempotency_records as record
set response = jsonb_set(
    record.response,
    '{route,gateway_scope_id}',
    to_jsonb(route.gateway_scope_id),
    true
)
from routes as route
where record.response #>> '{route,id}' = route.id::text;

update idempotency_records as record
set response = jsonb_set(
    record.response,
    '{cutover,routes}',
    (
        select jsonb_agg(
            document
                || jsonb_build_object(
                    'gateway_scope_id',
                    authoritative.gateway_scope_id
                )
            order by ordinality
        )
        from jsonb_array_elements(record.response #> '{cutover,routes}')
            with ordinality as candidate(document, ordinality)
        join routes as authoritative
            on authoritative.id::text = candidate.document ->> 'id'
    ),
    false
)
where jsonb_typeof(record.response #> '{cutover,routes}') = 'array';
