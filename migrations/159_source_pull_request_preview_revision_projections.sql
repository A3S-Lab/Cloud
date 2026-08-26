create table source_pull_request_preview_revision_projections (
    organization_id uuid not null,
    preview_id uuid not null,
    preview_aggregate_version bigint not null
        check (preview_aggregate_version >= 1),
    lifecycle_event_id uuid not null,
    correlation_id uuid not null,
    lifecycle_causation_id uuid not null,
    source_pull_request_change_id uuid not null,
    project_id uuid not null,
    source_environment_id uuid not null,
    source_subscription_id uuid not null,
    preview_environment_id uuid not null,
    installation_id bigint not null check (installation_id > 0),
    base_repository_identity text not null
        check (length(base_repository_identity) between 1 and 2048),
    base_branch text not null
        check (
            char_length(base_branch) between 1 and 255
            and base_branch !~ '^refs/'
            and base_branch !~ '^/'
            and base_branch !~ '/$'
            and base_branch !~ '[.]$'
            and base_branch !~ '[.][.]'
            and base_branch !~ '//'
            and base_branch <> '@'
            and base_branch !~ '(^|/)[.]'
            and base_branch !~ '[.](/|$)'
            and base_branch !~ '[.]lock(/|$)'
            and base_branch ~ '^[A-Za-z0-9._/-]+$'
        ),
    pull_request_id bigint not null check (pull_request_id > 0),
    pull_request_number bigint not null check (pull_request_number > 0),
    fact_digest text not null check (fact_digest ~ '^sha256:[0-9a-f]{64}$'),
    fact_occurred_at timestamptz not null,
    outcome text not null
        check (
            outcome in (
                'projected',
                'cleanup_required',
                'suppressed_inactive_subscription',
                'ignored_stale'
            )
        ),
    source_revision_id uuid,
    primary key (
        organization_id,
        preview_id,
        preview_aggregate_version
    ),
    unique (lifecycle_event_id),
    foreign key (lifecycle_event_id) references outbox_events (event_id),
    foreign key (organization_id, source_subscription_id)
        references github_repository_subscriptions (organization_id, id),
    foreign key (organization_id, source_revision_id)
        references external_source_revisions (organization_id, id),
    check (source_environment_id <> preview_environment_id),
    check (
        (outcome = 'projected' and source_revision_id is not null)
        or (outcome <> 'projected' and source_revision_id is null)
    )
);

create index source_preview_revision_projection_head_idx
    on source_pull_request_preview_revision_projections (
        organization_id,
        preview_id,
        preview_aggregate_version desc
    );

create function reject_source_pull_request_preview_revision_projection_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Preview Source revision projection receipts are immutable';
end
$$;

create trigger source_pull_request_preview_revision_projections_immutable
before update or delete on source_pull_request_preview_revision_projections
for each row execute function reject_source_pull_request_preview_revision_projection_mutation();

comment on table source_pull_request_preview_revision_projections is
    'Sources-owned append-only Preview version fence and immutable SourceRevision binding receipts; max Preview version is the head, while delivery and retry remain on the existing Outbox Relay, so this is not another Inbox, queue, worker, saga, or scheduler';

comment on column source_pull_request_preview_revision_projections.source_revision_id is
    'Ordinary Sources ExternalSourceRevision authority created only for the latest active Preview lifecycle version; its existing Environment foreign key proves the Projects handoff, while cleanup, suppression, and stale receipts need no Environment row and carry no revision';
