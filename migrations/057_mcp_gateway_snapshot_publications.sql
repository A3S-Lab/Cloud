alter table gateway_route_scopes
    add constraint gateway_route_scopes_id_tenant_environment_node_unique
    unique (
        id,
        organization_id,
        project_id,
        environment_id,
        node_id
    );

create table mcp_gateway_snapshot_publications (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    gateway_scope_id uuid not null,
    node_id uuid not null,
    gateway_revision bigint not null
        check (gateway_revision between 1 and 9007199254740991),
    gateway_command_id uuid not null,
    snapshot_digest text not null
        check (snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    staged_at timestamptz not null,
    primary key (node_id, gateway_revision),
    unique (node_id, gateway_command_id),
    foreign key (
        gateway_scope_id,
        organization_id,
        project_id,
        environment_id,
        node_id
    )
        references gateway_route_scopes (
            id,
            organization_id,
            project_id,
            environment_id,
            node_id
        ),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (
        node_id,
        gateway_revision,
        gateway_command_id
    )
        references gateway_publications (
            node_id,
            revision,
            command_id
        )
);

create index mcp_gateway_snapshot_publications_environment_idx
    on mcp_gateway_snapshot_publications (
        organization_id,
        project_id,
        environment_id,
        staged_at,
        node_id
    );

comment on table mcp_gateway_snapshot_publications is
    'Immutable kind and tenant evidence for complete hosted MCP Gateway publications; delivery state remains authoritative in gateway_publications';
