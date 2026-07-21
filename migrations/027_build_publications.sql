alter table build_runs
    add column publication_target jsonb,
    add column published_artifact jsonb;

with invalidated_builds as (
    update build_runs
    set status = 'failed',
        failure = 'build completed before OCI registry publication became authoritative; rebuild required',
        aggregate_version = aggregate_version + 1,
        updated_at = greatest(updated_at, statement_timestamp())
    where status = 'succeeded'
    returning operation_id, failure, updated_at
)
update operation_projections as projection
set status = 'failed',
    output = null,
    error = invalidated.failure,
    updated_at = invalidated.updated_at
from invalidated_builds as invalidated
where projection.operation_id = invalidated.operation_id;

update build_runs
set status = 'cleanup_pending',
    failure = 'build started before OCI registry publication became authoritative; rebuild required',
    aggregate_version = aggregate_version + 1,
    updated_at = greatest(updated_at, statement_timestamp())
where status in (
    'queued',
    'preparing',
    'prepared',
    'scheduled',
    'running',
    'validating',
    'cleanup_pending'
)
  and failure is null
  and cancellation_requested_at is null
  and exists (
      select 1
      from operation_requests as request
      where request.operation_id = build_runs.operation_id
        and request.workflow_name = 'cloud.build'
        and request.workflow_version = '1'
  );

alter table build_runs
    drop constraint build_runs_status_check,
    add constraint build_runs_status_check check (
        status in (
            'queued',
            'preparing',
            'prepared',
            'scheduled',
            'running',
            'validating',
            'publishing',
            'cancelling',
            'cleanup_pending',
            'succeeded',
            'failed',
            'cancelled'
        )
    ),
    add constraint build_runs_publication_target_check check (
        publication_target is null
        or (
            jsonb_typeof(publication_target) = 'object'
            and output is not null
            and publication_target -> 'descriptor' = output -> 'descriptor'
            and status in (
                'publishing',
                'cancelling',
                'cleanup_pending',
                'succeeded',
                'failed',
                'cancelled'
            )
        )
    ),
    add constraint build_runs_published_artifact_check check (
        published_artifact is null
        or (
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
                    || publication_target ->> 'registry'
                    || '/'
                    || publication_target ->> 'repository'
                    || '@'
                    || publication_target #>> '{descriptor,digest}'
        )
    ),
    add constraint build_runs_publishing_state_check check (
        status <> 'publishing'
        or (
            command_id is not null
            and runtime_output_artifact is not null
            and output is not null
            and publication_target is not null
            and cleanup_command_id is null
            and failure is null
            and cancellation_requested_at is null
        )
    ),
    add constraint build_runs_publication_cleanup_intent_check check (
        status <> 'cleanup_pending'
        or published_artifact is not null
        or failure is not null
        or cancellation_requested_at is not null
    ),
    add constraint build_runs_published_success_check check (
        status <> 'succeeded'
        or published_artifact is not null
    );
