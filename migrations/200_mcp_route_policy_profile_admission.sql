alter table mcp_route_policies
    add column profile_endpoint_path text,
    add column profile_max_request_bytes bigint,
    add column profile_max_response_bytes bigint,
    add column profile_max_stream_seconds bigint;

update mcp_route_policies as policy
set
    profile_endpoint_path = substring(profile.acl from 'endpoint_path[[:space:]]*=[[:space:]]*"([^"]+)"'),
    profile_max_request_bytes = (
        substring(profile.acl from 'max_request_bytes[[:space:]]*=[[:space:]]*([0-9]+)')
    )::bigint,
    profile_max_response_bytes = (
        substring(profile.acl from 'max_response_bytes[[:space:]]*=[[:space:]]*([0-9]+)')
    )::bigint,
    profile_max_stream_seconds = (
        substring(profile.acl from 'max_stream_seconds[[:space:]]*=[[:space:]]*([0-9]+)')
    )::bigint
from mcp_service_profiles as profile
where policy.organization_id = profile.organization_id
  and policy.asset_id = profile.asset_id
  and policy.asset_release_id = profile.asset_release_id
  and policy.profile_digest = profile.profile_digest;

alter table mcp_route_policies
    alter column profile_endpoint_path set not null,
    alter column profile_max_request_bytes set not null,
    alter column profile_max_response_bytes set not null,
    alter column profile_max_stream_seconds set not null,
    add constraint mcp_route_policies_profile_admission_check
        check (
            left(profile_endpoint_path, 1) = '/'
            and position('//' in profile_endpoint_path) = 0
            and octet_length(profile_endpoint_path) between 1 and 2048
            and profile_max_request_bytes between 1 and 9007199254740991
            and profile_max_response_bytes between 1 and 9007199254740991
            and profile_max_stream_seconds between 1 and 9007199254740991
        );

comment on column mcp_route_policies.profile_endpoint_path is
    'Edge-owned MCP profile endpoint-path admission fact retained with the route policy.';
comment on column mcp_route_policies.profile_max_request_bytes is
    'Edge-owned MCP profile max-request-bytes admission fact retained with the route policy.';
comment on column mcp_route_policies.profile_max_response_bytes is
    'Edge-owned MCP profile max-response-bytes admission fact retained with the route policy.';
comment on column mcp_route_policies.profile_max_stream_seconds is
    'Edge-owned MCP profile max-stream-seconds admission fact retained with the route policy.';
