alter table asset_releases
    add constraint asset_releases_agent_workload_identity_unique
    unique (
        organization_id,
        asset_id,
        id,
        build_run_id,
        artifact_digest,
        artifact_media_type
    );

alter table workload_revisions
    add column agent_organization_id uuid,
    add column agent_asset_id uuid,
    add column agent_asset_release_id uuid,
    add column agent_build_run_id uuid,
    add constraint workload_revisions_agent_binding_shape_check
        check (
            (
                agent_organization_id is null
                and agent_asset_id is null
                and agent_asset_release_id is null
                and agent_build_run_id is null
            )
            or (
                agent_organization_id is not null
                and agent_asset_id is not null
                and agent_asset_release_id is not null
                and agent_build_run_id is not null
                and resolution_state = 'resolved'
                and artifact_digest is not null
                and artifact_media_type is not null
                and external_build_run_id is null
                and mcp_asset_release_id is null
            )
        ),
    add constraint workload_revisions_agent_tenant_workload_fk
        foreign key (agent_organization_id, workload_id)
        references workloads (organization_id, id),
    add constraint workload_revisions_agent_exact_release_fk
        foreign key (
            agent_organization_id,
            agent_asset_id,
            agent_asset_release_id,
            agent_build_run_id,
            artifact_digest,
            artifact_media_type
        )
        references asset_releases (
            organization_id,
            asset_id,
            id,
            build_run_id,
            artifact_digest,
            artifact_media_type
        );

create index workload_revisions_agent_release_idx
    on workload_revisions (
        agent_organization_id,
        agent_asset_id,
        agent_asset_release_id,
        generation,
        id
    )
    where agent_asset_release_id is not null;

comment on column workload_revisions.agent_asset_release_id is
    'Exact published Agent release deployed through the ordinary Workload lifecycle';

comment on column workload_revisions.agent_build_run_id is
    'Successful hosted BuildRun whose immutable OCI publication backs the Agent revision';
