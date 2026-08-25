create table developer_pull_request_preview_policy_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    source_environment_id uuid not null,
    source_subscription_id uuid not null,
    id uuid not null,
    revision_number bigint not null check (revision_number >= 1),
    installation_id bigint not null check (installation_id > 0),
    repository_provider text not null check (repository_provider = 'github'),
    repository_url text not null,
    repository_identity text not null,
    base_branch text not null,
    policy_schema text not null
        check (policy_schema = 'a3s.cloud.pull-request-preview-policy.v1'),
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 16384),
    policy_digest text not null
        check (policy_digest ~ '^sha256:[0-9a-f]{64}$'),
    owner_principal_id uuid not null,
    lifetime_seconds bigint not null
        check (lifetime_seconds between 300 and 2592000),
    maximum_active_previews bigint not null
        check (maximum_active_previews between 1 and 256),
    fork_policy text not null check (fork_policy in ('deny', 'isolated')),
    allow_protected_secrets_for_trusted_sources boolean not null,
    maximum_workloads bigint not null
        check (maximum_workloads between 1 and 32),
    cpu_millis bigint not null check (cpu_millis between 1 and 128000),
    memory_bytes bigint not null
        check (
            memory_bytes between 67108864 and 549755813888
            and memory_bytes % 1048576 = 0
        ),
    ephemeral_storage_bytes bigint not null
        check (
            ephemeral_storage_bytes between 67108864 and 4398046511104
            and ephemeral_storage_bytes % 1048576 = 0
        ),
    accepted_by uuid not null,
    accepted_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, source_subscription_id, revision_number),
    unique (organization_id, source_subscription_id, id),
    foreign key (organization_id, project_id, source_environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, source_subscription_id)
        references github_repository_subscriptions (organization_id, id),
    constraint developer_preview_policy_owner_membership_fk
        foreign key (organization_id, owner_principal_id)
        references organization_memberships (organization_id, principal_id),
    constraint developer_preview_policy_actor_membership_fk
        foreign key (organization_id, accepted_by)
        references organization_memberships (organization_id, principal_id)
);

create index developer_preview_policy_current_idx
    on developer_pull_request_preview_policy_revisions (
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        revision_number desc
    );

create function validate_developer_preview_policy_revision()
returns trigger
language plpgsql
as $$
declare
    subscription_project_id uuid;
    subscription_environment_id uuid;
    subscription_installation_id bigint;
    subscription_repository_provider text;
    subscription_repository_url text;
    subscription_repository_identity text;
    subscription_branch_name text;
    subscription_status text;
    subscription_created_at timestamptz;
    previous_revision_number bigint;
    previous_accepted_at timestamptz;
    previous_project_id uuid;
    previous_environment_id uuid;
    previous_installation_id bigint;
    previous_repository_identity text;
    previous_base_branch text;
begin
    select project_id, environment_id, installation_id, repository_provider,
           repository_url, repository_identity, branch_name, status, created_at
      into subscription_project_id, subscription_environment_id,
           subscription_installation_id, subscription_repository_provider,
           subscription_repository_url, subscription_repository_identity,
           subscription_branch_name, subscription_status,
           subscription_created_at
      from github_repository_subscriptions
     where organization_id = new.organization_id
       and id = new.source_subscription_id;

    if not found
       or subscription_status <> 'active'
       or new.project_id <> subscription_project_id
       or new.source_environment_id <> subscription_environment_id
       or new.installation_id <> subscription_installation_id
       or new.repository_provider <> subscription_repository_provider
       or new.repository_url <> subscription_repository_url
       or new.repository_identity <> subscription_repository_identity
       or new.base_branch <> subscription_branch_name
       or new.accepted_at < subscription_created_at then
        raise exception 'Preview policy revision does not match its exact active source subscription';
    end if;

    select revision_number, accepted_at, project_id, source_environment_id,
           installation_id, repository_identity, base_branch
      into previous_revision_number, previous_accepted_at, previous_project_id,
           previous_environment_id, previous_installation_id,
           previous_repository_identity, previous_base_branch
      from developer_pull_request_preview_policy_revisions
     where organization_id = new.organization_id
       and source_subscription_id = new.source_subscription_id
     order by revision_number desc
     limit 1
     for share;

    if not found then
        if new.revision_number <> 1 then
            raise exception 'Preview policy revision sequence must start at one';
        end if;
    elsif new.revision_number <> previous_revision_number + 1
       or new.accepted_at < previous_accepted_at
       or new.project_id <> previous_project_id
       or new.source_environment_id <> previous_environment_id
       or new.installation_id <> previous_installation_id
       or new.repository_identity <> previous_repository_identity
       or new.base_branch <> previous_base_branch then
        raise exception 'Preview policy revision sequence is not monotonic';
    end if;
    return new;
end
$$;

create trigger developer_preview_policy_revision_validate
before insert on developer_pull_request_preview_policy_revisions
for each row execute function validate_developer_preview_policy_revision();

create function reject_developer_preview_policy_revision_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'accepted Preview policy revisions are immutable';
end
$$;

create trigger developer_preview_policy_revisions_immutable
before update or delete on developer_pull_request_preview_policy_revisions
for each row execute function reject_developer_preview_policy_revision_mutation();

comment on table developer_pull_request_preview_policy_revisions is
    'Developer Workflows-owned immutable pull-request Preview policy revisions; no Environment, SourceRevision, BuildRun, Workload, Route, Operation, timer, scheduler, webhook, or credential authority';
