alter table developer_pull_request_preview_policy_revisions
    add constraint developer_preview_policy_projection_reference_key
    unique (
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        id
    );

create index developer_preview_policy_effective_at_idx
    on developer_pull_request_preview_policy_revisions (
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        accepted_at desc,
        revision_number desc
    );

create table developer_pull_request_previews (
    organization_id uuid not null,
    project_id uuid not null,
    source_environment_id uuid not null,
    source_subscription_id uuid not null,
    id uuid not null,
    environment_id uuid not null,
    policy_revision_id uuid not null,
    pull_request_id bigint not null check (pull_request_id > 0),
    pull_request_number bigint not null check (pull_request_number > 0),
    head_repository_provider text,
    head_repository_url text,
    head_repository_identity text,
    head_branch text not null
        check (octet_length(head_branch) between 1 and 255),
    head_commit_sha text not null
        check (head_commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    provider_created_at timestamptz not null,
    last_provider_updated_at timestamptz not null,
    last_change_kind text not null
        check (last_change_kind in ('opened', 'synchronized', 'reopened', 'closed')),
    last_merged boolean not null,
    expires_at timestamptz not null,
    status text not null check (status in ('active', 'cleanup_required')),
    cleanup_reason text
        check (
            cleanup_reason is null
            or cleanup_reason in (
                'pull_request_closed',
                'pull_request_merged',
                'fork_denied',
                'expired'
            )
        ),
    cleanup_requested_at timestamptz,
    aggregate_version bigint not null check (aggregate_version >= 1),
    primary key (organization_id, id),
    unique (organization_id, source_subscription_id, pull_request_id),
    unique (organization_id, source_subscription_id, pull_request_number),
    foreign key (
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        policy_revision_id
    ) references developer_pull_request_preview_policy_revisions (
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        id
    ),
    check (
        (
            head_repository_provider is null
            and head_repository_url is null
            and head_repository_identity is null
        )
        or (
            head_repository_provider = 'github'
            and head_repository_url is not null
            and head_repository_identity is not null
        )
    ),
    check (provider_created_at <= last_provider_updated_at),
    check (expires_at > last_provider_updated_at),
    check (not last_merged or last_change_kind = 'closed'),
    check (
        (
            status = 'active'
            and last_change_kind <> 'closed'
            and cleanup_reason is null
            and cleanup_requested_at is null
        )
        or (
            status = 'cleanup_required'
            and cleanup_reason is not null
            and cleanup_requested_at is not null
        )
    ),
    check (
        status <> 'cleanup_required'
        or (
            cleanup_reason = 'pull_request_closed'
            and last_change_kind = 'closed'
            and not last_merged
            and cleanup_requested_at = last_provider_updated_at
        )
        or (
            cleanup_reason = 'pull_request_merged'
            and last_change_kind = 'closed'
            and last_merged
            and cleanup_requested_at = last_provider_updated_at
        )
        or (
            cleanup_reason = 'fork_denied'
            and last_change_kind <> 'closed'
            and cleanup_requested_at = last_provider_updated_at
        )
        or (
            cleanup_reason = 'expired'
            and last_change_kind <> 'closed'
            and cleanup_requested_at >= expires_at
        )
    )
);

create function validate_developer_pull_request_preview_mutation()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'pull-request Preview lifecycle projections cannot be deleted';
    end if;
    if new.organization_id <> old.organization_id
       or new.project_id <> old.project_id
       or new.source_environment_id <> old.source_environment_id
       or new.source_subscription_id <> old.source_subscription_id
       or new.id <> old.id
       or new.environment_id <> old.environment_id
       or new.policy_revision_id <> old.policy_revision_id
       or new.pull_request_id <> old.pull_request_id
       or new.pull_request_number <> old.pull_request_number
       or new.provider_created_at <> old.provider_created_at
       or new.aggregate_version <> old.aggregate_version + 1
       or new.last_provider_updated_at < old.last_provider_updated_at then
        raise exception 'pull-request Preview mutation changed immutable authority or skipped CAS';
    end if;
    return new;
end
$$;

create trigger developer_pull_request_previews_validate_mutation
before update or delete on developer_pull_request_previews
for each row execute function validate_developer_pull_request_preview_mutation();

create table developer_pull_request_change_projections (
    organization_id uuid not null,
    source_pull_request_change_id uuid not null,
    project_id uuid not null,
    source_environment_id uuid not null,
    source_subscription_id uuid not null,
    pull_request_id bigint not null check (pull_request_id > 0),
    pull_request_number bigint not null check (pull_request_number > 0),
    fact_digest text not null
        check (fact_digest ~ '^sha256:[0-9a-f]{64}$'),
    fact_occurred_at timestamptz not null,
    policy_revision_id uuid,
    preview_id uuid,
    preview_aggregate_version bigint
        check (preview_aggregate_version is null or preview_aggregate_version >= 1),
    outcome text not null
        check (
            outcome in (
                'no_applicable_policy',
                'created',
                'updated',
                'reactivated',
                'cleanup_required',
                'fork_denied',
                'ignored_duplicate',
                'ignored_stale'
            )
        ),
    primary key (organization_id, source_pull_request_change_id),
    foreign key (organization_id, source_subscription_id)
        references github_repository_subscriptions (organization_id, id),
    foreign key (
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        policy_revision_id
    ) references developer_pull_request_preview_policy_revisions (
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        id
    ),
    foreign key (organization_id, preview_id)
        references developer_pull_request_previews (organization_id, id),
    check ((preview_id is null) = (preview_aggregate_version is null)),
    check (
        (
            outcome = 'no_applicable_policy'
            and policy_revision_id is null
            and preview_id is null
        )
        or (
            outcome = 'fork_denied'
            and policy_revision_id is not null
        )
        or (
            outcome not in ('no_applicable_policy', 'fork_denied')
            and policy_revision_id is not null
            and preview_id is not null
        )
    )
);

create index developer_pull_request_change_projection_preview_idx
    on developer_pull_request_change_projections (
        organization_id,
        source_subscription_id,
        pull_request_id,
        fact_occurred_at
    );

create function reject_developer_pull_request_change_projection_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'pull-request change projection receipts are immutable';
end
$$;

create trigger developer_pull_request_change_projections_immutable
before update or delete on developer_pull_request_change_projections
for each row execute function reject_developer_pull_request_change_projection_mutation();

comment on table developer_pull_request_previews is
    'Developer Workflows-owned PR lifecycle projection bound to one immutable Preview Policy revision; it is not an Environment, BuildRun, Workload, Route, Operation, timer, or scheduler authority';

comment on table developer_pull_request_change_projections is
    'Developer Workflows-owned immutable local projection receipts; delivery and retry remain exclusively on the existing Outbox Relay, so this table is not another Inbox, queue, or worker';
