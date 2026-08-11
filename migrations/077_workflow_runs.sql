create table workflow_runs (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    workflow_goal_id uuid not null,
    plan_revision_id uuid not null,
    plan_digest text not null check (plan_digest ~ '^sha256:[0-9a-f]{64}$'),
    operation_id uuid not null,
    requested_by uuid not null references identity_principals(id),
    requested_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, id),
    unique (operation_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (
        organization_id,
        project_id,
        workflow_goal_id,
        plan_revision_id
    ) references workflow_plan_revisions (
        organization_id,
        project_id,
        workflow_goal_id,
        id
    ),
    foreign key (organization_id, operation_id)
        references operation_requests (organization_id, operation_id),
    check (id = operation_id)
);

create index workflow_runs_project_requested_idx
    on workflow_runs (organization_id, project_id, requested_at desc, id);

create trigger workflow_runs_immutable
before update or delete on workflow_runs
for each row execute function reject_workflow_immutable_mutation();

comment on table workflow_runs is
    'Immutable Workflow execution intent bound to one PlanRevision and the same-identity Operations/A3S Flow run';

