alter table build_runs
    add column evidence_required boolean not null default false,
    add column evidence jsonb;

alter table build_runs
    alter column evidence_required drop default,
    drop constraint build_runs_status_check,
    drop constraint build_runs_publication_target_check,
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
    add constraint build_runs_publication_target_check check (
        publication_target is null
        or (
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
        )
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
            and evidence ->> 'runtimeSpecDigest' = runtime_spec_digest
            and evidence -> 'artifact' = published_artifact
            and evidence ->> 'sbomDigest' ~ '^sha256:[0-9a-f]{64}$'
            and evidence ->> 'provenanceDigest' ~ '^sha256:[0-9a-f]{64}$'
            and jsonb_typeof(evidence -> 'sbom') = 'object'
            and jsonb_typeof(evidence -> 'provenance') = 'object'
            and jsonb_typeof(evidence -> 'envelope') = 'object'
            and jsonb_typeof(evidence -> 'signingKey') = 'object'
            and evidence #>> '{signingKey,algorithm}' = 'ed25519'
            and evidence #>> '{signingKey,keyId}' ~ '^sha256:[0-9a-f]{64}$'
            and evidence #>> '{signingKey,publicKey}' ~ '^[A-Za-z0-9+/]{43}=$'
            and evidence ->> 'verificationState' = 'verified'
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
    add constraint build_runs_attesting_state_check check (
        status <> 'attesting'
        or (
            command_id is not null
            and runtime_output_artifact is not null
            and output is not null
            and publication_target is not null
            and published_artifact is not null
            and evidence_required
            and cleanup_command_id is null
            and failure is null
        )
    ),
    add constraint build_runs_required_evidence_cleanup_check check (
        status not in ('cleanup_pending', 'succeeded', 'cancelled')
        or published_artifact is null
        or not evidence_required
        or evidence is not null
        or failure is not null
    ),
    add constraint build_runs_required_evidence_success_check check (
        status <> 'succeeded'
        or not evidence_required
        or evidence is not null
    ),
    add constraint build_runs_required_evidence_cancel_check check (
        status <> 'cancelled'
        or published_artifact is null
        or not evidence_required
        or evidence is not null
        or failure is not null
    );
