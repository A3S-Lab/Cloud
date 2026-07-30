alter table mcp_gateway_snapshot_publications
    add column publication_owner text not null default 'mcp-reconciler',
    add constraint mcp_gateway_snapshot_publications_owner_check
        check (publication_owner in ('mcp-reconciler', 'ordinary'));

comment on column mcp_gateway_snapshot_publications.publication_owner is
    'Selects the durable dispatcher: MCP reconciliation owns MCP-initiated publications, while the originating ordinary Route/rollout flow owns composed ordinary publications';
