alter table artifact_build_candidates
    add column preview_id uuid;

alter table artifact_build_candidates
    add constraint artifact_build_candidates_preview_shape_check
    check (
        preview_id is null
        or (
            subject_kind = 'external_source_revision'
            and preview_id <> '00000000-0000-0000-0000-000000000000'::uuid
        )
    );

create index artifact_build_candidates_preview_idx
    on artifact_build_candidates (organization_id, preview_id, requested_at)
    where preview_id is not null;

create table artifact_preview_build_lifecycle_projections (
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
    state text not null
        check (
            state in (
                'active',
                'cleanup_required',
                'suppressed_inactive_subscription'
            )
        ),
    source_revision_id uuid,
    repository_identity text,
    commit_sha text,
    recipe_digest text,
    source_revision_accepted_at timestamptz,
    fact_occurred_at timestamptz not null,
    outcome text not null check (outcome in ('applied', 'ignored_stale')),
    retirement text not null
        check (
            retirement in (
                'not_required',
                'pending_suppressed',
                'cancellation_requested',
                'terminal_observed'
            )
        ),
    retired_source_revision_id uuid,
    retired_build_run_id uuid,
    primary key (
        organization_id,
        preview_id,
        preview_aggregate_version
    ),
    unique (lifecycle_event_id),
    foreign key (lifecycle_event_id) references outbox_events (event_id),
    foreign key (organization_id, retired_build_run_id)
        references build_runs (organization_id, id),
    check (
        organization_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and preview_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and lifecycle_event_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and correlation_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and lifecycle_causation_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and source_pull_request_change_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and project_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and source_environment_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and source_subscription_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and preview_environment_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and (
            source_revision_id is null
            or source_revision_id <> '00000000-0000-0000-0000-000000000000'::uuid
        )
        and (
            retired_source_revision_id is null
            or retired_source_revision_id <> '00000000-0000-0000-0000-000000000000'::uuid
        )
        and (
            retired_build_run_id is null
            or retired_build_run_id <> '00000000-0000-0000-0000-000000000000'::uuid
        )
    ),
    check (source_environment_id <> preview_environment_id),
    check (
        (
            state = 'active'
            and source_revision_id is not null
            and repository_identity is not null
            and length(repository_identity) between 1 and 2048
            and commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'
            and recipe_digest ~ '^sha256:[0-9a-f]{64}$'
            and source_revision_accepted_at is not null
            and source_revision_accepted_at <= fact_occurred_at
        )
        or (
            state <> 'active'
            and source_revision_id is null
            and repository_identity is null
            and commit_sha is null
            and recipe_digest is null
            and source_revision_accepted_at is null
        )
    ),
    check (
        (retirement = 'not_required'
            and retired_source_revision_id is null
            and retired_build_run_id is null)
        or (retirement = 'pending_suppressed'
            and retired_source_revision_id is not null
            and retired_build_run_id is null)
        or (retirement in ('cancellation_requested', 'terminal_observed')
            and retired_source_revision_id is not null
            and retired_build_run_id is not null)
    ),
    check (
        retired_source_revision_id is null
        or retired_source_revision_id <> source_revision_id
    ),
    check (outcome <> 'ignored_stale' or retirement = 'not_required')
);

create index artifact_preview_build_lifecycle_head_idx
    on artifact_preview_build_lifecycle_projections (
        organization_id,
        preview_id,
        preview_aggregate_version desc
    );

create index artifact_preview_build_lifecycle_retry_authority_idx
    on artifact_preview_build_lifecycle_projections (
        organization_id,
        preview_id,
        retired_source_revision_id,
        retired_build_run_id,
        preview_aggregate_version desc
    )
    where retired_build_run_id is not null;

do $$
begin
    if exists (
        select 1
        from source_pull_request_preview_revision_projections s
        left join outbox_events e
            on e.event_key = 'source.pull-request-preview-revision.lifecycle-committed'
            and e.schema_version = 1
            and e.organization_id = s.organization_id
            and e.aggregate_id = s.preview_id
            and e.aggregate_version = s.preview_aggregate_version
            and e.causation_id = s.lifecycle_event_id
        where s.outcome <> 'ignored_stale'
        group by
            s.organization_id,
            s.preview_id,
            s.preview_aggregate_version
        having count(e.event_id) <> 1
    ) then
        raise exception 'every applied Preview SourceRevision receipt must have one exact specialized lifecycle fact';
    end if;

    if exists (
        select 1
        from outbox_events e
        left join source_pull_request_preview_revision_projections s
            on s.organization_id = e.organization_id
            and s.preview_id = e.aggregate_id
            and s.preview_aggregate_version = e.aggregate_version
            and s.lifecycle_event_id = e.causation_id
            and s.outcome <> 'ignored_stale'
        left join external_source_revisions r
            on r.organization_id = s.organization_id
            and r.id = s.source_revision_id
        where e.event_key = 'source.pull-request-preview-revision.lifecycle-committed'
            and (
                e.schema_version <> 1
                or e.event_id = '00000000-0000-0000-0000-000000000000'::uuid
                or e.correlation_id = '00000000-0000-0000-0000-000000000000'::uuid
                or e.causation_id is null
                or e.causation_id = '00000000-0000-0000-0000-000000000000'::uuid
                or s.organization_id is null
                or jsonb_typeof(e.payload) <> 'object'
                or octet_length(e.payload::text) > 16384
                or not (e.payload ?& array[
                    'source_pull_request_change_id',
                    'organization_id',
                    'project_id',
                    'source_environment_id',
                    'source_subscription_id',
                    'preview_id',
                    'preview_aggregate_version',
                    'preview_environment_id',
                    'state',
                    'source_revision_id',
                    'repository_identity',
                    'commit_sha',
                    'recipe_digest',
                    'source_revision_accepted_at'
                ])
                or (e.payload - array[
                    'source_pull_request_change_id',
                    'organization_id',
                    'project_id',
                    'source_environment_id',
                    'source_subscription_id',
                    'preview_id',
                    'preview_aggregate_version',
                    'preview_environment_id',
                    'state',
                    'source_revision_id',
                    'repository_identity',
                    'commit_sha',
                    'recipe_digest',
                    'source_revision_accepted_at'
                ]) <> '{}'::jsonb
                or (e.payload ->> 'source_pull_request_change_id')::uuid
                    is distinct from s.source_pull_request_change_id
                or (e.payload ->> 'organization_id')::uuid
                    is distinct from s.organization_id
                or (e.payload ->> 'project_id')::uuid
                    is distinct from s.project_id
                or (e.payload ->> 'source_environment_id')::uuid
                    is distinct from s.source_environment_id
                or (e.payload ->> 'source_subscription_id')::uuid
                    is distinct from s.source_subscription_id
                or (e.payload ->> 'preview_id')::uuid
                    is distinct from s.preview_id
                or (e.payload ->> 'preview_aggregate_version')::bigint
                    is distinct from s.preview_aggregate_version
                or (e.payload ->> 'preview_environment_id')::uuid
                    is distinct from s.preview_environment_id
                or e.occurred_at is distinct from s.fact_occurred_at
                or e.payload ->> 'state' is distinct from case s.outcome
                    when 'projected' then 'active'
                    when 'cleanup_required' then 'cleanup_required'
                    when 'suppressed_inactive_subscription' then
                        'suppressed_inactive_subscription'
                end
                or (e.payload ->> 'source_revision_id')::uuid
                    is distinct from s.source_revision_id
                or e.payload ->> 'repository_identity'
                    is distinct from r.repository_identity
                or e.payload ->> 'commit_sha' is distinct from r.commit_sha
                or e.payload ->> 'recipe_digest' is distinct from r.recipe_digest
                or (e.payload ->> 'source_revision_accepted_at')::timestamptz
                    is distinct from r.accepted_at
            )
    ) then
        raise exception 'stored Preview SourceRevision lifecycle fact drifted from its Sources receipt';
    end if;
end
$$;

with lifecycle as (
    select
        e.organization_id,
        e.aggregate_id as preview_id,
        e.aggregate_version as preview_aggregate_version,
        e.event_id as lifecycle_event_id,
        e.correlation_id,
        e.causation_id as lifecycle_causation_id,
        s.source_pull_request_change_id,
        s.project_id,
        s.source_environment_id,
        s.source_subscription_id,
        s.preview_environment_id,
        case s.outcome
            when 'projected' then 'active'
            when 'cleanup_required' then 'cleanup_required'
            when 'suppressed_inactive_subscription' then
                'suppressed_inactive_subscription'
        end as state,
        s.source_revision_id,
        r.repository_identity,
        r.commit_sha,
        r.recipe_digest,
        r.accepted_at as source_revision_accepted_at,
        e.occurred_at as fact_occurred_at,
        lag(s.source_revision_id) over (
            partition by e.organization_id, e.aggregate_id
            order by e.aggregate_version
        ) as previous_source_revision_id
    from outbox_events e
    join source_pull_request_preview_revision_projections s
        on s.organization_id = e.organization_id
        and s.preview_id = e.aggregate_id
        and s.preview_aggregate_version = e.aggregate_version
        and s.lifecycle_event_id = e.causation_id
        and s.outcome <> 'ignored_stale'
    left join external_source_revisions r
        on r.organization_id = s.organization_id
        and r.id = s.source_revision_id
    where e.event_key = 'source.pull-request-preview-revision.lifecycle-committed'
        and e.schema_version = 1
)
insert into artifact_preview_build_lifecycle_projections (
    organization_id,
    preview_id,
    preview_aggregate_version,
    lifecycle_event_id,
    correlation_id,
    lifecycle_causation_id,
    source_pull_request_change_id,
    project_id,
    source_environment_id,
    source_subscription_id,
    preview_environment_id,
    state,
    source_revision_id,
    repository_identity,
    commit_sha,
    recipe_digest,
    source_revision_accepted_at,
    fact_occurred_at,
    outcome,
    retirement,
    retired_source_revision_id,
    retired_build_run_id
)
select
    organization_id,
    preview_id,
    preview_aggregate_version,
    lifecycle_event_id,
    correlation_id,
    lifecycle_causation_id,
    source_pull_request_change_id,
    project_id,
    source_environment_id,
    source_subscription_id,
    preview_environment_id,
    state,
    source_revision_id,
    repository_identity,
    commit_sha,
    recipe_digest,
    source_revision_accepted_at,
    fact_occurred_at,
    'applied',
    case
        when previous_source_revision_id is not null
            and previous_source_revision_id is distinct from source_revision_id
        then 'pending_suppressed'
        else 'not_required'
    end,
    case
        when previous_source_revision_id is not null
            and previous_source_revision_id is distinct from source_revision_id
        then previous_source_revision_id
    end,
    null
from lifecycle;

do $$
begin
    if exists (
        select 1
        from build_runs b
        join artifact_preview_build_lifecycle_projections p
            on p.organization_id = b.organization_id
            and (
                p.source_revision_id = b.source_revision_id
                or p.retired_source_revision_id = b.source_revision_id
            )
    ) then
        raise exception 'pre-upgrade Preview SourceRevision already has a BuildRun without lifecycle retirement authority';
    end if;

    if exists (
        select 1
        from artifact_preview_build_lifecycle_projections p
        where p.source_revision_id is not null
        group by p.organization_id, p.source_revision_id
        having count(distinct p.preview_id) <> 1
    ) then
        raise exception 'one Preview SourceRevision was associated with multiple Preview identities';
    end if;

    if exists (
        select 1
        from artifact_preview_build_lifecycle_projections p
        join external_source_revisions r
            on r.organization_id = p.organization_id
            and r.id = p.source_revision_id
        join artifact_build_candidates c
            on c.organization_id = r.organization_id
            and c.subject_kind = 'external_source_revision'
            and c.subject_id = r.id
        where p.state = 'active'
            and (
                c.preview_id is not null and c.preview_id <> p.preview_id
                or c.project_id is distinct from r.project_id
                or c.environment_id is distinct from r.environment_id
                or c.source_revision_id is distinct from r.id
                or c.repository_identity is distinct from r.repository_identity
                or c.commit_sha is distinct from r.commit_sha
                or c.owner_input_digest is distinct from r.recipe_digest
                or c.requested_at is distinct from r.accepted_at
            )
    ) then
        raise exception 'pre-upgrade Preview build candidate conflicts with Sources evidence';
    end if;
end
$$;

update artifact_build_candidates c
set preview_id = p.preview_id
from artifact_preview_build_lifecycle_projections p
where p.state = 'active'
    and c.organization_id = p.organization_id
    and c.subject_kind = 'external_source_revision'
    and c.subject_id = p.source_revision_id
    and c.preview_id is null;

insert into artifact_build_candidates (
    organization_id,
    subject_kind,
    subject_id,
    preview_id,
    project_id,
    environment_id,
    source_revision_id,
    asset_id,
    asset_release_id,
    repository_identity,
    commit_sha,
    owner_input_digest,
    requested_at
)
select distinct
    p.organization_id,
    'external_source_revision',
    p.source_revision_id,
    p.preview_id,
    p.project_id,
    p.preview_environment_id,
    p.source_revision_id,
    null::uuid,
    null::uuid,
    p.repository_identity,
    p.commit_sha,
    p.recipe_digest,
    p.source_revision_accepted_at
from artifact_preview_build_lifecycle_projections p
where p.state = 'active'
on conflict (organization_id, subject_kind, subject_id) do nothing;

create function reject_artifact_build_candidate_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Artifact build candidate fact projections are immutable';
end
$$;

create trigger artifact_build_candidates_immutable
before update or delete on artifact_build_candidates
for each row execute function reject_artifact_build_candidate_mutation();

create function reject_artifact_preview_build_lifecycle_projection_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Preview build lifecycle projection receipts are immutable';
end
$$;

create trigger artifact_preview_build_lifecycle_projections_immutable
before update or delete on artifact_preview_build_lifecycle_projections
for each row execute function reject_artifact_preview_build_lifecycle_projection_mutation();

comment on table artifact_preview_build_lifecycle_projections is
    'Artifacts-owned append-only Preview version fence and immutable build admission/retirement receipt; max Preview version is the head and delivery/retry remain on the existing Outbox Relay, so this is not an Inbox, queue, worker, saga, scheduler, or second BuildRun lifecycle';

comment on column artifact_build_candidates.preview_id is
    'Optional immutable Preview provenance; reservation remains governed by the latest Artifacts-local lifecycle receipt and BuildRun remains the sole executable build lifecycle';

comment on column artifact_preview_build_lifecycle_projections.retired_build_run_id is
    'Exact prior BuildRun whose cancellation or terminal observation authorizes at most one same-SourceRevision retry after a later active lifecycle version';
