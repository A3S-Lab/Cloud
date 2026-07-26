alter table gateway_route_scopes
    add column membership_generation bigint not null default 1
        check (membership_generation > 0),
    add column min_ready integer not null default 1
        check (min_ready > 0 and min_ready <= 100),
    add column max_unavailable integer not null default 0
        check (max_unavailable >= 0 and max_unavailable < 100);

alter table gateway_route_scopes
    add constraint gateway_route_scopes_membership_tenant_key
        unique (id, organization_id, project_id, environment_id);

create table gateway_scope_members (
    gateway_scope_id uuid not null,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    node_id uuid not null,
    ordinal integer not null check (ordinal >= 0 and ordinal < 100),
    membership_generation bigint not null check (membership_generation > 0),
    added_at timestamptz not null,
    primary key (gateway_scope_id, node_id),
    unique (gateway_scope_id, ordinal),
    unique (organization_id, project_id, environment_id, node_id),
    foreign key (
        gateway_scope_id,
        organization_id,
        project_id,
        environment_id
    )
        references gateway_route_scopes (
            id,
            organization_id,
            project_id,
            environment_id
        ),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id)
);

insert into gateway_scope_members (
    gateway_scope_id,
    organization_id,
    project_id,
    environment_id,
    node_id,
    ordinal,
    membership_generation,
    added_at
)
select
    id,
    organization_id,
    project_id,
    environment_id,
    node_id,
    0,
    membership_generation,
    created_at
from gateway_route_scopes;

update idempotency_records
set response = response || jsonb_build_object(
    'member_node_ids',
    jsonb_build_array(response -> 'node_id'),
    'membership_generation',
    1,
    'rollout_policy',
    jsonb_build_object(
        'min_ready',
        1,
        'max_unavailable',
        0
    )
)
where scope_key ~ '^organizations/[^/]+/projects/[^/]+/environments/[^/]+/gateway-scopes$'
    and jsonb_typeof(response) = 'object'
    and response ? 'node_id'
    and not response ? 'member_node_ids';

alter table gateway_route_scopes
    add constraint gateway_route_scopes_primary_member_fk
        foreign key (id, node_id)
        references gateway_scope_members (gateway_scope_id, node_id)
        deferrable initially deferred;

create index gateway_scope_members_node_idx
    on gateway_scope_members (
        organization_id,
        node_id,
        gateway_scope_id
    );
