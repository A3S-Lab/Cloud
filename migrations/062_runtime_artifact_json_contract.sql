alter table build_runs
    drop constraint build_runs_box_output_shape_check,
    drop constraint build_runs_validated_output_check;

alter table build_runs
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
            and box_build_output #>> '{artifact,artifact,media_type}'
                = 'application/vnd.a3s.directory.v1+tar'
            and jsonb_typeof(box_build_output #> '{artifact,size_bytes}') = 'number'
            and (box_build_output #>> '{artifact,size_bytes}')::numeric
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
                = box_build_output #>> '{artifact,artifact,media_type}'
            and output #> '{artifact,sizeBytes}'
                = box_build_output #> '{artifact,size_bytes}'
            and output -> 'descriptor' = box_build_output -> 'descriptor'
            and output -> 'contentBytes' = box_build_output -> 'contentBytes'
            and output -> 'blobCount' = box_build_output -> 'blobCount'
            and jsonb_typeof(output -> 'platforms') = 'array'
            and jsonb_array_length(output -> 'platforms')
                = jsonb_array_length(box_build_output -> 'platforms')
        ), false)
    );

comment on constraint build_runs_box_output_shape_check on build_runs is
    'Validates the canonical mixed JSON contract: Cloud Box receipt fields are camelCase while nested A3S Runtime artifact fields are snake_case';
