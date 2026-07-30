alter table mcp_gateway_snapshot_publications
    drop constraint mcp_gateway_snapshot_publications_scope_node_fk,
    add constraint mcp_gateway_snapshot_publications_scope_tenant_fk
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
        );

-- Membership is checked under lock when a snapshot is staged. Keeping the
-- immutable publication marker independent of the mutable membership table
-- lets a node leave a logical scope without deleting its publication history.

alter table mcp_gateway_snapshot_publications
    add column desired_state_digest text,
    add column mcp_route_count integer;

update mcp_gateway_snapshot_publications
set
    desired_state_digest = snapshot_digest,
    mcp_route_count = 1
where desired_state_digest is null
    or mcp_route_count is null;

alter table mcp_gateway_snapshot_publications
    alter column desired_state_digest set not null,
    alter column mcp_route_count set not null,
    add constraint mcp_gateway_snapshot_publications_desired_state_digest_check
        check (desired_state_digest ~ '^sha256:[0-9a-f]{64}$'),
    add constraint mcp_gateway_snapshot_publications_mcp_route_count_check
        check (mcp_route_count between 0 and 1000);

comment on column mcp_gateway_snapshot_publications.desired_state_digest is
    'Stable digest of logical scope, ordinary routes, hosted MCP policy/targets/credential authority, and compiler configuration; excludes physical revision, command, certificate identity, and observation time';

comment on column mcp_gateway_snapshot_publications.mcp_route_count is
    'Number of hosted MCP routes in this complete physical snapshot; zero is durable removal evidence and prevents the MCP reconciler from owning later ordinary-only changes';
