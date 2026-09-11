alter table workload_revisions
    add column mcp_runtime_port text,
    add column mcp_health_path text;

update workload_revisions as revision
set
    mcp_runtime_port = substring(profile.acl from 'runtime_port[[:space:]]*=[[:space:]]*"([^"]+)"'),
    mcp_health_path = substring(profile.acl from 'health_path[[:space:]]*=[[:space:]]*"([^"]+)"')
from mcp_service_profiles as profile
where revision.mcp_organization_id = profile.organization_id
  and revision.mcp_asset_id = profile.asset_id
  and revision.mcp_asset_release_id = profile.asset_release_id
  and revision.mcp_profile_digest = profile.profile_digest
  and revision.mcp_profile_digest is not null;

alter table workload_revisions
    drop constraint workload_revisions_mcp_binding_shape_check;

alter table workload_revisions
    add constraint workload_revisions_mcp_binding_shape_check
        check (
            (
                mcp_organization_id is null
                and mcp_asset_id is null
                and mcp_asset_release_id is null
                and mcp_profile_digest is null
                and mcp_runtime_port is null
                and mcp_health_path is null
            )
            or (
                mcp_organization_id is not null
                and mcp_asset_id is not null
                and mcp_asset_release_id is not null
                and mcp_profile_digest is not null
                and mcp_runtime_port is not null
                and octet_length(mcp_runtime_port) between 1 and 63
                and mcp_health_path is not null
                and left(mcp_health_path, 1) = '/'
                and position('//' in mcp_health_path) = 0
                and resolution_state = 'resolved'
                and artifact_digest is not null
                and artifact_media_type is not null
            )
        );

comment on column workload_revisions.mcp_runtime_port is
    'Workloads-owned MCP profile Runtime port admission fact retained with the revision binding.';
comment on column workload_revisions.mcp_health_path is
    'Workloads-owned MCP profile health-path admission fact retained with the revision binding.';
