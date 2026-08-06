create table mcp_gateway_snapshot_publication_scopes (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    gateway_scope_id uuid not null,
    node_id uuid not null,
    gateway_revision bigint not null
        check (gateway_revision between 1 and 9007199254740991),
    scope_aggregate_version bigint not null
        check (scope_aggregate_version between 1 and 9007199254740991),
    membership_generation bigint not null
        check (membership_generation between 1 and 9007199254740991),
    receiving_member boolean not null,
    mcp_route_count integer not null
        check (mcp_route_count between 0 and 1000),
    primary key (node_id, gateway_revision, gateway_scope_id),
    foreign key (node_id, gateway_revision)
        references mcp_gateway_snapshot_publications (node_id, gateway_revision),
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
        )
);

insert into mcp_gateway_snapshot_publication_scopes (
    organization_id,
    project_id,
    environment_id,
    gateway_scope_id,
    node_id,
    gateway_revision,
    scope_aggregate_version,
    membership_generation,
    receiving_member,
    mcp_route_count
)
select
    marker.organization_id,
    marker.project_id,
    marker.environment_id,
    marker.gateway_scope_id,
    marker.node_id,
    marker.gateway_revision,
    scope.aggregate_version,
    scope.membership_generation,
    exists (
        select 1
        from gateway_scope_members member
        where member.gateway_scope_id = marker.gateway_scope_id
          and member.node_id = marker.node_id
    ),
    marker.mcp_route_count
from mcp_gateway_snapshot_publications marker
join gateway_route_scopes scope
  on scope.id = marker.gateway_scope_id
 and scope.organization_id = marker.organization_id
 and scope.project_id = marker.project_id
 and scope.environment_id = marker.environment_id;

create index mcp_gateway_snapshot_publication_scopes_scope_head_idx
    on mcp_gateway_snapshot_publication_scopes (
        gateway_scope_id,
        node_id,
        gateway_revision desc
    );

create index mcp_gateway_snapshot_publication_scopes_node_head_idx
    on mcp_gateway_snapshot_publication_scopes (
        node_id,
        gateway_scope_id,
        gateway_revision desc
    );

comment on table mcp_gateway_snapshot_publication_scopes is
    'Immutable logical-scope CAS and per-scope route-count evidence for one complete physical MCP Gateway publication';

comment on column mcp_gateway_snapshot_publications.gateway_scope_id is
    'Deterministic primary logical scope for dispatch compatibility; the complete scope set is authoritative in mcp_gateway_snapshot_publication_scopes';

comment on column mcp_gateway_snapshot_publications.desired_state_digest is
    'Stable digest of the complete logical scope set, ordinary routes, hosted MCP policy/targets/credential authority, and compiler configuration; excludes physical revision, command, certificate identity, and observation time';
