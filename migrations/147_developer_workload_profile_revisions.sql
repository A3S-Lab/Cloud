create table developer_workload_profile_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    profile_id uuid not null,
    id uuid not null,
    revision_number bigint not null check (revision_number >= 1),
    build_plan_id uuid not null,
    source_revision_id uuid not null,
    project_root text not null
        check (octet_length(project_root) between 1 and 4096),
    profile_name text not null
        check (
            octet_length(profile_name) between 1 and 63
            and profile_name ~ '^[a-z]([a-z0-9-]{0,61}[a-z0-9])?$'
        ),
    profile_kind text not null
        check (profile_kind in ('web', 'worker', 'scheduled_task')),
    contract_schema text not null
        check (contract_schema = 'a3s.cloud.workload-profile.v1'),
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 131072),
    contract_digest text not null
        check (contract_digest ~ '^sha256:[0-9a-f]{64}$'),
    build_plan_digest text not null
        check (build_plan_digest ~ '^sha256:[0-9a-f]{64}$'),
    accepted_by uuid not null references identity_principals(id),
    accepted_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, profile_id, revision_number),
    unique (organization_id, profile_id, id),
    foreign key (
        organization_id,
        build_plan_id,
        build_plan_digest
    ) references developer_build_plans (
        organization_id,
        id,
        contract_digest
    ),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id)
);

create index developer_workload_profile_current_idx
    on developer_workload_profile_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_number desc
    );

create index developer_workload_profile_build_plan_idx
    on developer_workload_profile_revisions (
        organization_id,
        project_id,
        environment_id,
        build_plan_id,
        profile_name,
        revision_number
    );

create function validate_developer_workload_profile_revision()
returns trigger
language plpgsql
as $$
declare
    plan_project_id uuid;
    plan_environment_id uuid;
    plan_source_revision_id uuid;
    plan_project_root text;
    plan_accepted_at timestamptz;
    previous_revision_number bigint;
    previous_accepted_at timestamptz;
    previous_project_root text;
    previous_profile_name text;
begin
    select project_id, environment_id, source_revision_id, project_root, accepted_at
      into plan_project_id, plan_environment_id, plan_source_revision_id,
           plan_project_root, plan_accepted_at
      from developer_build_plans
     where organization_id = new.organization_id
       and id = new.build_plan_id
       and contract_digest = new.build_plan_digest;

    if not found
       or new.project_id <> plan_project_id
       or new.environment_id <> plan_environment_id
       or new.source_revision_id <> plan_source_revision_id
       or new.project_root <> plan_project_root
       or new.accepted_at < plan_accepted_at then
        raise exception 'accepted workload profile revision does not match its exact BuildPlan';
    end if;

    select revision_number, accepted_at, project_root, profile_name
      into previous_revision_number, previous_accepted_at,
           previous_project_root, previous_profile_name
      from developer_workload_profile_revisions
     where organization_id = new.organization_id
       and profile_id = new.profile_id
     order by revision_number desc
     limit 1
     for share;

    if not found then
        if new.revision_number <> 1 then
            raise exception 'workload profile revision sequence must start at one';
        end if;
    elsif new.revision_number <> previous_revision_number + 1
       or new.accepted_at < previous_accepted_at
       or new.project_root <> previous_project_root
       or new.profile_name <> previous_profile_name then
        raise exception 'workload profile revision sequence is not monotonic';
    end if;
    return new;
end
$$;

create trigger developer_workload_profile_revision_validate
before insert on developer_workload_profile_revisions
for each row execute function validate_developer_workload_profile_revision();

create function reject_developer_workload_profile_revision_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'accepted workload profile revisions are immutable';
end
$$;

create trigger developer_workload_profile_revisions_immutable
before update or delete on developer_workload_profile_revisions
for each row execute function reject_developer_workload_profile_revision_mutation();

comment on table developer_workload_profile_revisions is
    'Developer Workflows-owned immutable accepted workload profile revisions; no BuildRun, Workload, Route, Execution, Automation, or scheduler authority';
