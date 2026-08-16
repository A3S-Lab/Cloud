alter table build_runs
    add column published_output jsonb;

alter table build_runs
    add constraint build_runs_published_output_check check (
        published_output is null
        or coalesce((
            jsonb_typeof(published_output) = 'object'
            and published_output ?& array['uri', 'digest', 'mediaType', 'sizeBytes']
            and published_output - array['uri', 'digest', 'mediaType', 'sizeBytes']
                = '{}'::jsonb
            and published_output ->> 'digest' ~ '^sha256:[0-9a-f]{64}$'
            and published_output ->> 'uri'
                = 'a3s-cloud-artifact://sha256/'
                    || substring(published_output ->> 'digest' from 8)
            and published_output ->> 'mediaType' in (
                'application/vnd.a3s.skill.bundle.v1+tar',
                'application/vnd.a3s.durable-cell.bundle.v1+tar'
            )
            and jsonb_typeof(published_output -> 'sizeBytes') = 'number'
            and (published_output ->> 'sizeBytes')::numeric > 0
            and published_artifact is not null
            and published_output ->> 'digest' <> published_artifact ->> 'digest'
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
    add constraint build_runs_published_output_evidence_check check (
        evidence is null
        or coalesce((
            jsonb_typeof(evidence #> '{provenance,subject}') = 'array'
            and (
                (
                    published_output is null
                    and jsonb_array_length(evidence #> '{provenance,subject}') = 2
                    and not (
                        evidence #> '{provenance,predicate,buildDefinition,internalParameters}'
                        ? 'publishedOutput'
                    )
                )
                or (
                    published_output is not null
                    and jsonb_array_length(evidence #> '{provenance,subject}') = 3
                    and evidence #> '{provenance,predicate,buildDefinition,internalParameters,publishedOutput}'
                        = published_output
                    and evidence #> '{provenance,subject}' @> jsonb_build_array(
                        jsonb_build_object(
                            'name', published_output ->> 'uri',
                            'digest', jsonb_build_object(
                                'sha256',
                                substring(published_output ->> 'digest' from 8)
                            )
                        )
                    )
                )
            )
        ), false)
    );

comment on column build_runs.published_output is
    'Optional single typed content-addressed output of the existing BuildRun; not an OCI manifest alias or a Durable Cells lifecycle authority';

comment on constraint build_runs_published_output_check on build_runs is
    'Keeps typed BuildRun outputs immutable, content-addressed, and distinct from the OCI publication';

comment on constraint build_runs_published_output_evidence_check on build_runs is
    'Requires an optional typed output to be an exact additional subject of the existing signed provenance';
