alter table mcp_gateway_snapshot_publications
    add column desired_gateway_scope_ids jsonb;

update mcp_gateway_snapshot_publications
set desired_gateway_scope_ids = case
    when mcp_route_count = 0 then '[]'::jsonb
    else jsonb_build_array(gateway_scope_id)
end
where desired_gateway_scope_ids is null;

alter table mcp_gateway_snapshot_publications
    alter column desired_gateway_scope_ids set not null,
    add constraint mcp_gateway_snapshot_publications_desired_scope_ids_check
        check (
            jsonb_typeof(desired_gateway_scope_ids) = 'array'
            and jsonb_array_length(desired_gateway_scope_ids) <= 1000
            and (
                jsonb_array_length(desired_gateway_scope_ids) = 0
                or desired_gateway_scope_ids ->> 0 = gateway_scope_id::text
            )
            and (
                mcp_route_count = 0
                or jsonb_array_length(desired_gateway_scope_ids) > 0
            )
        );

comment on column mcp_gateway_snapshot_publications.gateway_scope_id is
    'Deterministic logical-scope anchor for this physical-node publication; desired_gateway_scope_ids is the complete active MCP scope set';

comment on column mcp_gateway_snapshot_publications.desired_gateway_scope_ids is
    'Canonical sorted logical Gateway scope IDs whose active MCP routes were composed into this complete physical-node snapshot; an empty array is removal evidence';

create table mcp_gateway_snapshot_heads (
    organization_id uuid not null,
    node_id uuid primary key,
    gateway_revision bigint not null
        check (gateway_revision between 1 and 9007199254740991),
    advanced_at timestamptz not null,
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (node_id, gateway_revision)
        references mcp_gateway_snapshot_publications (
            node_id,
            gateway_revision
        )
);

insert into mcp_gateway_snapshot_heads (
    organization_id,
    node_id,
    gateway_revision,
    advanced_at
)
select
    latest.organization_id,
    latest.node_id,
    latest.gateway_revision,
    latest.staged_at
from (
    select distinct on (marker.node_id)
        marker.organization_id,
        marker.node_id,
        marker.gateway_revision,
        marker.mcp_route_count,
        marker.staged_at,
        publication.state
    from mcp_gateway_snapshot_publications marker
    inner join gateway_publications publication
        on publication.node_id = marker.node_id
        and publication.revision = marker.gateway_revision
    order by marker.node_id, marker.gateway_revision desc
) latest
where latest.mcp_route_count > 0
    or latest.state <> 'applied';

create index mcp_gateway_snapshot_heads_organization_idx
    on mcp_gateway_snapshot_heads (
        organization_id,
        node_id
    );

comment on table mcp_gateway_snapshot_heads is
    'One mutable pointer from each physical Gateway node to its latest MCP-owned complete snapshot; successful zero-route acknowledgement releases the pointer while immutable publication history remains';
