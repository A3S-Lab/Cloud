create table connector_execution_evidence (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    profile_id uuid not null,
    revision_id uuid not null,
    attempt_id uuid not null,
    request_digest text not null
        check (request_digest ~ '^sha256:[0-9a-f]{64}$'),
    request_body_bytes bigint not null
        check (request_body_bytes between 0 and 1048576),
    outcome text not null
        check (outcome in ('accepted', 'retryable', 'rejected')),
    response_status integer
        check (response_status between 100 and 599),
    response_digest text
        check (response_digest ~ '^sha256:[0-9a-f]{64}$'),
    response_body_bytes bigint
        check (response_body_bytes between 0 and 1048576),
    retry_after_seconds bigint
        check (retry_after_seconds between 0 and 86400),
    started_at timestamptz not null,
    completed_at timestamptz not null,
    primary key (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        attempt_id
    ),
    foreign key (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id
    ) references connector_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        id
    ),
    check (completed_at >= started_at),
    check (
        outcome = 'accepted'
        and response_status between 200 and 299
        and response_digest is not null
        and response_body_bytes is not null
        and retry_after_seconds is null
        or outcome = 'retryable'
        and (response_status is null or response_status not between 200 and 299)
        and response_digest is null
        and response_body_bytes is null
        or outcome = 'rejected'
        and (response_status is null or response_status not between 200 and 299)
        and response_digest is null
        and response_body_bytes is null
        and retry_after_seconds is null
    )
);

create index connector_execution_evidence_revision_feed_idx
    on connector_execution_evidence (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        completed_at desc,
        attempt_id desc
    );

create function reject_connector_execution_evidence_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Connector execution evidence is immutable';
end
$$;

create trigger connector_execution_evidence_immutable
before update or delete on connector_execution_evidence
for each row execute function reject_connector_execution_evidence_mutation();

comment on table connector_execution_evidence is
    'Connectors-owned immutable bounded terminal facts; not an execution queue, attempt reservation, retry store, scheduler, provider response store, or caller acknowledgement authority';

comment on column connector_execution_evidence.request_digest is
    'SHA-256 binding of the bounded caller-owned request; request headers, bodies, signing input, endpoints, addresses, and credentials are never stored';

comment on column connector_execution_evidence.response_digest is
    'Accepted bounded response-body digest only; provider response bytes and text are never stored';
