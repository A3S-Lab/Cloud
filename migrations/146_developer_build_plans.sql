create table developer_build_plans (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    source_revision_id uuid not null,
    project_root text not null
        check (octet_length(project_root) between 1 and 4096),
    contract_schema text not null
        check (contract_schema = 'a3s.cloud.build-plan.v1'),
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 65536),
    contract_digest text not null
        check (contract_digest ~ '^sha256:[0-9a-f]{64}$'),
    proposal_digest text not null
        check (proposal_digest ~ '^sha256:[0-9a-f]{64}$'),
    source_identity_digest text not null
        check (source_identity_digest ~ '^sha256:[0-9a-f]{64}$'),
    commit_sha text not null
        check (commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    source_content_digest text not null
        check (source_content_digest ~ '^sha256:[0-9a-f]{64}$'),
    detector_kind text not null
        check (detector_kind in ('asset_acl', 'dockerfile')),
    detector_revision text not null
        check (detector_revision = 'p0.1-c1'),
    evidence_path text not null
        check (octet_length(evidence_path) between 1 and 4096),
    evidence_digest text not null
        check (evidence_digest ~ '^sha256:[0-9a-f]{64}$'),
    recipe_digest text not null
        check (recipe_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version = 1),
    accepted_by uuid not null references identity_principals(id),
    accepted_at timestamptz not null,
    primary key (organization_id, id),
    unique (
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        project_root
    ),
    unique (organization_id, id, contract_digest),
    foreign key (
        organization_id,
        project_id,
        environment_id,
        source_revision_id
    ) references external_source_revisions (
        organization_id,
        project_id,
        environment_id,
        id
    ),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id)
);

create index developer_build_plans_source_idx
    on developer_build_plans (
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        project_root,
        id
    );

create index developer_build_plans_acceptance_idx
    on developer_build_plans (
        organization_id,
        project_id,
        environment_id,
        accepted_at,
        id
    );

create function validate_developer_build_plan_source()
returns trigger
language plpgsql
as $$
declare
    stored_repository_identity text;
    stored_commit_sha text;
    stored_recipe_digest text;
    stored_accepted_at timestamptz;
begin
    select repository_identity, commit_sha, recipe_digest, accepted_at
      into stored_repository_identity, stored_commit_sha,
           stored_recipe_digest, stored_accepted_at
      from external_source_revisions
     where organization_id = new.organization_id
       and project_id = new.project_id
       and environment_id = new.environment_id
       and id = new.source_revision_id;

    if not found
       or new.source_identity_digest <> (
           'sha256:' || encode(
               sha256(convert_to(
                   stored_repository_identity || E'\n' || stored_commit_sha,
                   'UTF8'
               )),
               'hex'
           )
       )
       or new.commit_sha <> stored_commit_sha
       or new.recipe_digest <> stored_recipe_digest
       or new.accepted_at < stored_accepted_at then
        raise exception 'accepted BuildPlan does not match its exact Source revision';
    end if;
    return new;
end
$$;

create trigger developer_build_plans_validate_source
before insert on developer_build_plans
for each row execute function validate_developer_build_plan_source();

create function reject_developer_build_plan_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'accepted BuildPlans are immutable';
end
$$;

create trigger developer_build_plans_immutable
before update or delete on developer_build_plans
for each row execute function reject_developer_build_plan_mutation();

comment on table developer_build_plans is
    'Developer Workflows-owned immutable accepted BuildPlans bound to exact Sources-owned revisions; no BuildRun, Workload, Route, or scheduler authority';
