with invalidated_builds as (
    update build_runs
    set status = 'failed',
        node_id = null,
        command_id = null,
        cleanup_command_id = null,
        runtime_spec_digest = null,
        runtime_output_artifact = null,
        output = null,
        publication_target = null,
        published_artifact = null,
        evidence = null,
        cache = null,
        cache_required = false,
        cancellation_requested_at = null,
        failure = 'build predates the sole Box-native workflow; rebuild required',
        aggregate_version = aggregate_version + 1,
        updated_at = greatest(updated_at, statement_timestamp()),
        finished_at = greatest(
            coalesce(finished_at, requested_at),
            statement_timestamp()
        )
    returning operation_id, failure, updated_at
)
insert into operation_projections (
    operation_id,
    status,
    last_sequence,
    output,
    error,
    updated_at
)
select
    invalidated.operation_id,
    'cancelled',
    coalesce(projection.last_sequence, 0),
    null,
    invalidated.failure,
    invalidated.updated_at
from invalidated_builds as invalidated
join operation_requests as request
  on request.operation_id = invalidated.operation_id
left join operation_projections as projection
  on projection.operation_id = invalidated.operation_id
on conflict (operation_id) do update
set status = excluded.status,
    output = excluded.output,
    error = excluded.error,
    updated_at = excluded.updated_at;

do $$
declare
    constraint_name text;
begin
    for constraint_name in
        select conname
        from pg_constraint
        where conrelid = 'build_runs'::regclass
          and contype = 'c'
    loop
        execute format(
            'alter table build_runs drop constraint %I',
            constraint_name
        );
    end loop;
end
$$;

alter table build_runs
    rename column runtime_spec_digest to build_request_digest;

alter table build_runs
    rename column runtime_output_artifact to box_build_output;

alter table build_runs
    drop column cache_required,
    drop column cache;

alter table build_runs
    add constraint build_runs_status_check check (
        status in (
            'queued',
            'preparing',
            'prepared',
            'scheduled',
            'running',
            'validating',
            'publishing',
            'attesting',
            'cancelling',
            'cleanup_pending',
            'succeeded',
            'failed',
            'cancelled'
        )
    ),
    add constraint build_runs_version_check check (
        id = operation_id
        and aggregate_version > 0
        and attempt > 0
        and (
            (attempt = 1 and retry_of_build_run_id is null)
            or (attempt > 1 and retry_of_build_run_id is not null)
        )
    ),
    add constraint build_runs_time_check check (
        updated_at >= requested_at
        and (started_at is null or started_at >= requested_at)
        and (
            cancellation_requested_at is null
            or cancellation_requested_at >= requested_at
        )
        and (finished_at is null or finished_at >= requested_at)
        and ((status in ('succeeded', 'failed', 'cancelled')) = (finished_at is not null))
    ),
    add constraint build_runs_input_check check (
        (
            source_content_digest is null
            and input_artifact is null
        )
        or coalesce((
            source_content_digest ~ '^sha256:[0-9a-f]{64}$'
            and jsonb_typeof(input_artifact) = 'object'
            and input_artifact ->> 'digest' ~ '^sha256:[0-9a-f]{64}$'
        ), false)
    ),
    add constraint build_runs_box_chain_check check (
        (node_id is null) = (build_request_digest is null)
        and (
            build_request_digest is null
            or build_request_digest ~ '^sha256:[0-9a-f]{64}$'
        )
        and (node_id is null or input_artifact is not null)
        and (command_id is null or node_id is not null)
        and (cleanup_command_id is null or command_id is not null)
        and (box_build_output is null or command_id is not null)
        and (input_artifact is null or started_at is not null)
    ),
    add constraint build_runs_box_output_shape_check check (
        box_build_output is null
        or coalesce((
            jsonb_typeof(box_build_output) = 'object'
            and box_build_output #>> '{artifact,name}' = 'oci-layout'
            and jsonb_typeof(box_build_output #> '{artifact,artifact}') = 'object'
            and box_build_output #>> '{artifact,artifact,uri}'
                = 'a3s-cloud-artifact://sha256/'
                    || substr(box_build_output #>> '{artifact,artifact,digest}', 8)
            and box_build_output #>> '{artifact,artifact,digest}'
                ~ '^sha256:[0-9a-f]{64}$'
            and box_build_output #>> '{artifact,artifact,mediaType}'
                = 'application/vnd.a3s.directory.v1+tar'
            and jsonb_typeof(box_build_output #> '{artifact,sizeBytes}') = 'number'
            and (box_build_output #>> '{artifact,sizeBytes}')::numeric
                between 1 and 10737418240
            and jsonb_typeof(box_build_output -> 'descriptor') = 'object'
            and box_build_output #>> '{descriptor,digest}'
                ~ '^sha256:[0-9a-f]{64}$'
            and box_build_output #>> '{descriptor,mediaType}' in (
                'application/vnd.oci.image.index.v1+json',
                'application/vnd.oci.image.manifest.v1+json'
            )
            and jsonb_typeof(box_build_output #> '{descriptor,size}') = 'number'
            and (box_build_output #>> '{descriptor,size}')::numeric > 0
            and jsonb_typeof(box_build_output -> 'platforms') = 'array'
            and jsonb_array_length(box_build_output -> 'platforms') between 1 and 8
            and jsonb_typeof(box_build_output -> 'manifestCount') = 'number'
            and (box_build_output ->> 'manifestCount')::numeric
                = jsonb_array_length(box_build_output -> 'platforms')
            and jsonb_typeof(box_build_output -> 'contentBytes') = 'number'
            and (box_build_output ->> 'contentBytes')::numeric
                >= (box_build_output #>> '{descriptor,size}')::numeric
            and jsonb_typeof(box_build_output -> 'blobCount') = 'number'
            and (box_build_output ->> 'blobCount')::numeric >= 2
            and jsonb_typeof(box_build_output -> 'caches') = 'array'
            and jsonb_array_length(box_build_output -> 'caches')
                = jsonb_array_length(box_build_output -> 'platforms')
            and box_build_output ->> 'blobInventoryDigest'
                ~ '^sha256:[0-9a-f]{64}$'
        ), false)
    ),
    add constraint build_runs_validated_output_check check (
        output is null
        or coalesce((
            box_build_output is not null
            and jsonb_typeof(output) = 'object'
            and output #>> '{artifact,uri}'
                = box_build_output #>> '{artifact,artifact,uri}'
            and output #>> '{artifact,digest}'
                = box_build_output #>> '{artifact,artifact,digest}'
            and output #>> '{artifact,mediaType}'
                = box_build_output #>> '{artifact,artifact,mediaType}'
            and output #> '{artifact,sizeBytes}'
                = box_build_output #> '{artifact,sizeBytes}'
            and output -> 'descriptor' = box_build_output -> 'descriptor'
            and output -> 'contentBytes' = box_build_output -> 'contentBytes'
            and output -> 'blobCount' = box_build_output -> 'blobCount'
            and jsonb_typeof(output -> 'platforms') = 'array'
            and jsonb_array_length(output -> 'platforms')
                = jsonb_array_length(box_build_output -> 'platforms')
        ), false)
    ),
    add constraint build_runs_publication_target_check check (
        publication_target is null
        or coalesce((
            jsonb_typeof(publication_target) = 'object'
            and output is not null
            and publication_target -> 'descriptor' = output -> 'descriptor'
            and status in (
                'publishing',
                'attesting',
                'cancelling',
                'cleanup_pending',
                'succeeded',
                'failed',
                'cancelled'
            )
        ), false)
    ),
    add constraint build_runs_published_artifact_check check (
        published_artifact is null
        or coalesce((
            jsonb_typeof(published_artifact) = 'object'
            and publication_target is not null
            and published_artifact ->> 'digest'
                = publication_target #>> '{descriptor,digest}'
            and published_artifact ->> 'mediaType'
                = publication_target #>> '{descriptor,mediaType}'
            and published_artifact -> 'sizeBytes'
                = publication_target #> '{descriptor,size}'
            and published_artifact ->> 'uri'
                = 'oci://'
                    || (publication_target ->> 'registry')
                    || '/'
                    || (publication_target ->> 'repository')
                    || '@'
                    || (publication_target #>> '{descriptor,digest}')
        ), false)
    ),
    add constraint build_runs_evidence_shape_check check (
        evidence is null
        or coalesce((
            evidence_required
            and jsonb_typeof(evidence) = 'object'
            and evidence ->> 'schema' = 'a3s.cloud.build-evidence.v1'
            and evidence ->> 'buildRunId' = id::text
            and evidence ->> 'operationId' = operation_id::text
            and evidence ->> 'sourceRevisionId' = source_revision_id::text
            and evidence -> 'attempt' = to_jsonb(attempt)
            and evidence ->> 'sourceContentDigest' = source_content_digest
            and evidence ->> 'buildRequestDigest' = build_request_digest
            and evidence -> 'artifact' = published_artifact
            and evidence -> 'platforms' = output -> 'platforms'
            and evidence ->> 'recipeDigest' ~ '^sha256:[0-9a-f]{64}$'
            and evidence ->> 'sbomDigest' ~ '^sha256:[0-9a-f]{64}$'
            and evidence ->> 'provenanceDigest' ~ '^sha256:[0-9a-f]{64}$'
            and jsonb_typeof(evidence -> 'builder') = 'object'
            and evidence #>> '{builder,digest}' ~ '^sha256:[0-9a-f]{64}$'
            and jsonb_typeof(evidence -> 'sbom') = 'object'
            and jsonb_typeof(evidence -> 'provenance') = 'object'
            and jsonb_typeof(evidence -> 'envelope') = 'object'
            and jsonb_typeof(evidence -> 'signingKey') = 'object'
            and evidence ->> 'verificationState' = 'verified'
            and evidence #>> '{signingKey,algorithm}' = 'ed25519'
            and evidence #>> '{signingKey,keyId}' ~ '^sha256:[0-9a-f]{64}$'
            and evidence #>> '{signingKey,publicKey}' ~ '^[A-Za-z0-9+/]{43}=$'
            and octet_length(evidence::text) <= 67108864
            and status in (
                'attesting',
                'cancelling',
                'cleanup_pending',
                'succeeded',
                'failed',
                'cancelled'
            )
        ), false)
    ),
    add constraint build_runs_failure_check check (
        failure is null or octet_length(failure) between 1 and 16384
    ),
    add constraint build_runs_terminal_cleanup_check check (
        status not in ('succeeded', 'failed', 'cancelled')
        or command_id is null
        or cleanup_command_id is not null
    ),
    add constraint build_runs_success_check check (
        status <> 'succeeded'
        or (
            published_artifact is not null
            and cleanup_command_id is not null
            and failure is null
            and cancellation_requested_at is null
            and (not evidence_required or evidence is not null)
        )
    ),
    add constraint build_runs_failed_check check (
        status <> 'failed'
        or (failure is not null and cancellation_requested_at is null)
    ),
    add constraint build_runs_cancelled_check check (
        status <> 'cancelled'
        or (
            cancellation_requested_at is not null
            and (
                published_artifact is null
                or not evidence_required
                or evidence is not null
                or failure is not null
            )
        )
    ),
    add constraint build_runs_execution_state_check check (
        (status <> 'queued' or (
            started_at is null
            and input_artifact is null
            and node_id is null
            and command_id is null
            and cleanup_command_id is null
            and box_build_output is null
            and output is null
            and publication_target is null
            and published_artifact is null
            and evidence is null
            and failure is null
            and cancellation_requested_at is null
        ))
        and (status <> 'preparing' or (
            started_at is not null
            and input_artifact is null
            and node_id is null
            and command_id is null
            and cleanup_command_id is null
            and box_build_output is null
            and output is null
            and publication_target is null
            and published_artifact is null
            and evidence is null
            and failure is null
            and cancellation_requested_at is null
        ))
        and (status <> 'prepared' or (
            started_at is not null
            and input_artifact is not null
            and node_id is null
            and command_id is null
            and cleanup_command_id is null
            and box_build_output is null
            and output is null
            and publication_target is null
            and published_artifact is null
            and evidence is null
            and failure is null
            and cancellation_requested_at is null
        ))
        and (status <> 'scheduled' or (
            input_artifact is not null
            and node_id is not null
            and command_id is null
            and cleanup_command_id is null
            and box_build_output is null
            and output is null
            and publication_target is null
            and published_artifact is null
            and evidence is null
            and failure is null
            and cancellation_requested_at is null
        ))
        and (status <> 'running' or (
            command_id is not null
            and cleanup_command_id is null
            and box_build_output is null
            and output is null
            and publication_target is null
            and published_artifact is null
            and evidence is null
            and failure is null
            and cancellation_requested_at is null
        ))
        and (status <> 'validating' or (
            command_id is not null
            and box_build_output is not null
            and cleanup_command_id is null
            and publication_target is null
            and published_artifact is null
            and evidence is null
            and failure is null
            and cancellation_requested_at is null
        ))
        and (status <> 'publishing' or (
            command_id is not null
            and box_build_output is not null
            and output is not null
            and publication_target is not null
            and cleanup_command_id is null
            and evidence is null
            and failure is null
            and cancellation_requested_at is null
        ))
        and (status <> 'attesting' or (
            command_id is not null
            and box_build_output is not null
            and output is not null
            and publication_target is not null
            and published_artifact is not null
            and evidence_required
            and cleanup_command_id is null
            and failure is null
        ))
        and (status <> 'cancelling' or cancellation_requested_at is not null)
        and (status <> 'cleanup_pending' or (
            published_artifact is not null
            or failure is not null
            or cancellation_requested_at is not null
        ))
    ),
    add constraint build_runs_required_evidence_cleanup_check check (
        status not in ('cleanup_pending', 'succeeded', 'cancelled')
        or published_artifact is null
        or not evidence_required
        or evidence is not null
        or failure is not null
    );
