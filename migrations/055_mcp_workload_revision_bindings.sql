alter table asset_releases
    add constraint asset_releases_exact_oci_artifact_unique
    unique (
        organization_id,
        asset_id,
        id,
        artifact_digest,
        artifact_media_type
    );

alter table workload_revisions
    add column mcp_organization_id uuid,
    add column mcp_asset_id uuid,
    add column mcp_asset_release_id uuid,
    add column mcp_profile_digest text,
    add constraint workload_revisions_mcp_profile_digest_check
        check (
            mcp_profile_digest is null
            or mcp_profile_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    add constraint workload_revisions_mcp_binding_shape_check
        check (
            (
                mcp_organization_id is null
                and mcp_asset_id is null
                and mcp_asset_release_id is null
                and mcp_profile_digest is null
            )
            or (
                mcp_organization_id is not null
                and mcp_asset_id is not null
                and mcp_asset_release_id is not null
                and mcp_profile_digest is not null
                and resolution_state = 'resolved'
                and artifact_digest is not null
                and artifact_media_type is not null
            )
        ),
    add constraint workload_revisions_mcp_tenant_workload_fk
        foreign key (mcp_organization_id, workload_id)
        references workloads (organization_id, id),
    add constraint workload_revisions_mcp_profile_fk
        foreign key (
            mcp_organization_id,
            mcp_asset_id,
            mcp_asset_release_id,
            mcp_profile_digest
        )
        references mcp_service_profiles (
            organization_id,
            asset_id,
            asset_release_id,
            profile_digest
        ),
    add constraint workload_revisions_mcp_exact_artifact_fk
        foreign key (
            mcp_organization_id,
            mcp_asset_id,
            mcp_asset_release_id,
            artifact_digest,
            artifact_media_type
        )
        references asset_releases (
            organization_id,
            asset_id,
            id,
            artifact_digest,
            artifact_media_type
        );

create index workload_revisions_mcp_release_idx
    on workload_revisions (
        mcp_organization_id,
        mcp_asset_id,
        mcp_asset_release_id,
        generation,
        id
    )
    where mcp_asset_release_id is not null;

comment on column workload_revisions.mcp_profile_digest is
    'Opaque immutable hosted MCP semantics profile projected into the ordinary Runtime Service specification';
