alter table gateway_route_scopes
    add constraint gateway_route_scopes_id_tenant_environment_unique
    unique (id, organization_id, project_id, environment_id);

alter table workloads
    add constraint workloads_tenant_environment_id_unique
    unique (organization_id, project_id, environment_id, id);

create table mcp_route_policies (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    gateway_scope_id uuid not null,
    workload_id uuid not null,
    asset_id uuid not null,
    asset_release_id uuid not null,
    profile_digest text not null
        check (profile_digest ~ '^sha256:[0-9a-f]{64}$'),
    hostname text not null,
    path text not null,
    policy_revision bigint not null
        check (
            policy_revision between 1 and 9007199254740991
        ),
    policy_digest text not null
        check (policy_digest ~ '^sha256:[0-9a-f]{64}$'),
    acl text not null
        check (octet_length(acl) between 1 and 524288),
    expires_at timestamptz not null,
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, id),
    unique (gateway_scope_id, hostname, path),
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
    foreign key (
        organization_id,
        project_id,
        environment_id,
        workload_id
    )
        references workloads (
            organization_id,
            project_id,
            environment_id,
            id
        ),
    foreign key (
        organization_id,
        asset_id,
        asset_release_id,
        profile_digest
    )
        references mcp_service_profiles (
            organization_id,
            asset_id,
            asset_release_id,
            profile_digest
        ),
    check (updated_at >= created_at),
    check (expires_at > updated_at)
);

create index mcp_route_policies_environment_idx
    on mcp_route_policies (
        organization_id,
        project_id,
        environment_id,
        updated_at,
        id
    );

create index mcp_route_policies_expiry_idx
    on mcp_route_policies (expires_at, id);

comment on table mcp_route_policies is
    'Mutable hosted MCP Edge desired state; Runtime targets and credential verifiers are resolved only into complete Gateway snapshots';
