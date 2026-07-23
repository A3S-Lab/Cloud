alter table build_runs
    add column cache_required boolean not null default false,
    add column cache jsonb;

alter table build_runs
    alter column cache_required drop default,
    add constraint build_runs_cache_shape_check check (
        cache is null
        or coalesce((
            cache_required
            and jsonb_typeof(cache) = 'object'
            and cache ->> 'schema' = 'a3s.cloud.build-cache.v1'
            and cache ->> 'key' ~ '^sha256:[0-9a-f]{64}$'
            and jsonb_typeof(cache -> 'artifact') = 'object'
            and cache -> 'artifact' = runtime_output_artifact
            and cache -> 'artifact' = output -> 'artifact'
            and jsonb_typeof(cache -> 'descriptor') = 'object'
            and cache #>> '{descriptor,digest}' ~ '^sha256:[0-9a-f]{64}$'
            and jsonb_typeof(cache -> 'contentBytes') = 'number'
            and (cache ->> 'contentBytes')::numeric > 0
            and jsonb_typeof(cache -> 'blobCount') = 'number'
            and (cache ->> 'blobCount')::numeric > 0
        ), false)
    ),
    add constraint build_runs_required_cache_check check (
        not cache_required
        or output is null
        or cache is not null
    );
