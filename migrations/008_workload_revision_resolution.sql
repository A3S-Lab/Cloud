alter table workload_revisions
    add column resolution_state text not null default 'resolved',
    add column artifact_source_uri text,
    add column expected_artifact_digest text,
    add column template_request jsonb,
    add column request_digest text,
    add column resolved_at timestamptz;

update workload_revisions
set artifact_source_uri = artifact_uri,
    expected_artifact_digest = artifact_digest,
    template_request = jsonb_set(
        template,
        '{artifact}',
        jsonb_build_object(
            'uri', artifact_uri,
            'expected_digest', artifact_digest
        )
    ),
    resolved_at = created_at;

create function a3s_cloud_canonical_json(input jsonb)
returns text
language plpgsql
immutable
strict
as $$
declare
    output text;
    item record;
    separator text := '';
begin
    case jsonb_typeof(input)
        when 'object' then
            output := '{';
            for item in select key, value from jsonb_each(input) order by key loop
                output := output || separator || to_jsonb(item.key)::text || ':'
                    || a3s_cloud_canonical_json(item.value);
                separator := ',';
            end loop;
            return output || '}';
        when 'array' then
            output := '[';
            for item in select value from jsonb_array_elements(input) loop
                output := output || separator || a3s_cloud_canonical_json(item.value);
                separator := ',';
            end loop;
            return output || ']';
        else
            return input::text;
    end case;
end;
$$;

update workload_revisions
set request_digest = 'sha256:' || encode(
    sha256(convert_to(a3s_cloud_canonical_json(template_request), 'UTF8')),
    'hex'
);

drop function a3s_cloud_canonical_json(jsonb);

alter table workload_revisions
    alter column artifact_source_uri set not null,
    alter column template_request set not null,
    alter column request_digest set not null,
    alter column artifact_uri drop not null,
    alter column artifact_digest drop not null,
    alter column artifact_media_type drop not null,
    alter column template drop not null,
    alter column template_digest drop not null,
    drop constraint workload_revisions_artifact_uri_check,
    drop constraint workload_revisions_artifact_digest_check,
    drop constraint workload_revisions_template_digest_check,
    add constraint workload_revisions_resolution_state_check check (
        resolution_state in ('pending', 'resolved')
    ),
    add constraint workload_revisions_source_uri_check check (
        artifact_source_uri like 'oci://%'
    ),
    add constraint workload_revisions_expected_digest_check check (
        expected_artifact_digest is null
        or expected_artifact_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint workload_revisions_request_digest_check check (
        request_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint workload_revisions_resolution_check check (
        (
            resolution_state = 'pending'
            and artifact_uri is null
            and artifact_digest is null
            and artifact_media_type is null
            and template is null
            and template_digest is null
            and resolved_at is null
        )
        or (
            resolution_state = 'resolved'
            and artifact_uri is not null
            and artifact_digest is not null
            and artifact_media_type is not null
            and template is not null
            and template_digest is not null
            and resolved_at is not null
        )
    ),
    add constraint workload_revisions_resolved_uri_check check (
        artifact_uri is null or artifact_uri like 'oci://%@sha256:%'
    ),
    add constraint workload_revisions_resolved_digest_check check (
        artifact_digest is null or artifact_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint workload_revisions_resolved_template_digest_check check (
        template_digest is null or template_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint workload_revisions_resolution_time_check check (
        resolved_at is null or resolved_at >= created_at
    );

alter table workload_revisions
    alter column resolution_state drop default;
