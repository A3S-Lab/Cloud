alter table build_runs
    drop constraint build_runs_source_attempt_unique,
    drop constraint build_runs_evidence_shape_check,
    add column subject_kind text not null default 'external_source_revision',
    add column asset_id uuid,
    add column asset_release_id uuid;

alter table build_runs
    alter column subject_kind drop default,
    alter column project_id drop not null,
    alter column environment_id drop not null,
    alter column source_revision_id drop not null,
    add constraint build_runs_subject_shape_check check (
        (
            subject_kind = 'external_source_revision'
            and project_id is not null
            and environment_id is not null
            and source_revision_id is not null
            and asset_id is null
            and asset_release_id is null
        )
        or (
            subject_kind = 'asset_release'
            and project_id is null
            and environment_id is null
            and source_revision_id is null
            and asset_id is not null
            and asset_release_id is not null
        )
    ),
    add constraint build_runs_asset_release_foreign_key foreign key (
        organization_id,
        asset_id,
        asset_release_id
    ) references asset_releases (
        organization_id,
        asset_id,
        id
    ),
    add constraint build_runs_evidence_shape_check check (
        evidence is null
        or coalesce((
            evidence_required
            and jsonb_typeof(evidence) = 'object'
            and evidence ->> 'schema' = 'a3s.cloud.build-evidence.v1'
            and evidence ->> 'buildRunId' = id::text
            and evidence ->> 'operationId' = operation_id::text
            and (
                (
                    subject_kind = 'external_source_revision'
                    and evidence ->> 'sourceRevisionId' = source_revision_id::text
                    and not (evidence ? 'assetId')
                    and not (evidence ? 'assetReleaseId')
                    and not (evidence ? 'manifestDigest')
                )
                or (
                    subject_kind = 'asset_release'
                    and evidence ->> 'assetId' = asset_id::text
                    and evidence ->> 'assetReleaseId' = asset_release_id::text
                    and evidence ->> 'manifestDigest' ~ '^sha256:[0-9a-f]{64}$'
                    and not (evidence ? 'sourceRevisionId')
                )
            )
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
    );

drop index build_runs_source_attempt_idx;

create unique index build_runs_external_subject_attempt_unique
    on build_runs (organization_id, source_revision_id, attempt)
    where subject_kind = 'external_source_revision';

create unique index build_runs_asset_release_attempt_unique
    on build_runs (organization_id, asset_release_id, attempt)
    where subject_kind = 'asset_release';

comment on column build_runs.subject_kind is
    'Closed build identity discriminator; exactly one external revision or hosted Asset release owns the BuildRun';

comment on constraint build_runs_asset_release_foreign_key on build_runs is
    'Binds hosted BuildRuns to the exact tenant-qualified immutable Asset release identity';
