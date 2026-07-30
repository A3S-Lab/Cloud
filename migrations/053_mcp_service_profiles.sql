alter table asset_releases
    add constraint asset_releases_organization_asset_id_id_unique
    unique (organization_id, asset_id, id);

create table mcp_service_profiles (
    organization_id uuid not null,
    asset_id uuid not null,
    asset_release_id uuid not null,
    profile_digest text not null
        check (profile_digest ~ '^sha256:[0-9a-f]{64}$'),
    acl text not null check (octet_length(acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, asset_release_id),
    unique (
        organization_id,
        asset_id,
        asset_release_id,
        profile_digest
    ),
    foreign key (organization_id, asset_id, asset_release_id)
        references asset_releases (organization_id, asset_id, id)
);

create index mcp_service_profiles_asset_idx
    on mcp_service_profiles (
        organization_id,
        asset_id,
        created_at,
        asset_release_id
    );

comment on table mcp_service_profiles is
    'Immutable canonical A3S ACL behavior profile bound to one published MCP AssetRelease';
