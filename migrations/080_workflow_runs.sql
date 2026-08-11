create table workflow_runs (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    workflow_goal_id uuid not null,
    plan_revision_id uuid not null,
    plan_digest text not null check (plan_digest ~ '^sha256:[0-9a-f]{64}$'),
    operation_id uuid not null,
    flow_run_id text not null check (char_length(flow_run_id) between 1 and 255),
    flow_runtime_build_id text check (
        flow_runtime_build_id is null
        or char_length(flow_runtime_build_id) between 1 and 255
    ),
    execution_input text not null check (
        octet_length(execution_input) between 1 and 8388608
    ),
    execution_input_digest text not null check (
        execution_input_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    status text not null check (
        status in (
            'pending',
            'running',
            'waiting',
            'cancelling',
            'completed',
            'failed',
            'cancelled',
            'timed_out'
        )
    ),
    last_flow_sequence bigint not null check (last_flow_sequence >= 0),
    output jsonb check (
        output is null or octet_length(output::text) <= 262144
    ),
    output_digest text check (
        output_digest is null or output_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    error text check (error is null or octet_length(error) between 1 and 16384),
    aggregate_version bigint not null check (aggregate_version > 0),
    requested_by uuid not null references identity_principals(id),
    requested_at timestamptz not null,
    updated_at timestamptz not null,
    started_at timestamptz,
    cancellation_requested_at timestamptz,
    cancellation_reason text check (
        cancellation_reason is null
        or octet_length(cancellation_reason) between 1 and 4096
    ),
    finished_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, project_id, id),
    unique (organization_id, operation_id),
    unique (organization_id, flow_run_id),
    foreign key (organization_id, project_id, workflow_goal_id)
        references workflow_goals (organization_id, project_id, id),
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
    check (id = operation_id),
    check (flow_run_id = id::text),
    check ((output is null) = (output_digest is null)),
    check ((status = 'completed') = (output is not null)),
    check ((status in ('failed', 'timed_out')) = (error is not null)),
    check ((status in ('completed', 'failed', 'cancelled', 'timed_out')) = (finished_at is not null)),
    check (status <> 'cancelling' or cancellation_requested_at is not null),
    check (cancellation_reason is null or cancellation_requested_at is not null),
    check (updated_at >= requested_at),
    check (started_at is null or started_at >= requested_at),
    check (
        cancellation_requested_at is null
        or cancellation_requested_at >= requested_at
    ),
    check (finished_at is null or finished_at >= requested_at)
);

create table workflow_step_projections (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_run_id uuid not null,
    step_id text not null check (
        step_id ~ '^[A-Za-z][A-Za-z0-9_-]{0,127}$'
    ),
    kind text not null check (
        kind in ('input', 'transform', 'branch', 'output')
    ),
    status text not null check (
        status in ('pending', 'running', 'completed', 'failed', 'cancelled', 'skipped')
    ),
    flow_step_id text not null check (
        octet_length(flow_step_id) between 10 and 137
    ),
    attempt_generation integer not null check (attempt_generation >= 0),
    selected_handle text check (
        selected_handle is null
        or selected_handle ~ '^[A-Za-z][A-Za-z0-9_-]{0,127}$'
    ),
    result jsonb check (
        result is null or octet_length(result::text) <= 262144
    ),
    result_digest text check (
        result_digest is null or result_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    error text check (error is null or octet_length(error) between 1 and 16384),
    evidence_references jsonb not null default '[]'::jsonb check (
        jsonb_typeof(evidence_references) = 'array'
        and jsonb_array_length(evidence_references) <= 32
    ),
    last_flow_sequence bigint not null check (last_flow_sequence >= 0),
    updated_at timestamptz not null,
    primary key (organization_id, workflow_run_id, step_id),
    foreign key (organization_id, project_id, workflow_run_id)
        references workflow_runs (organization_id, project_id, id),
    check (flow_step_id = 'workflow:' || step_id),
    check ((result is null) = (result_digest is null)),
    check ((status = 'completed') = (result is not null)),
    check ((status = 'failed') = (error is not null)),
    check (kind = 'branch' or selected_handle is null)
);

create index workflow_runs_project_requested_idx
    on workflow_runs (organization_id, project_id, requested_at desc, id desc);

create index workflow_runs_reconciliation_idx
    on workflow_runs (updated_at, id)
    where status not in ('completed', 'failed', 'cancelled', 'timed_out');

create index workflow_step_projections_run_status_idx
    on workflow_step_projections (organization_id, workflow_run_id, status, step_id);

create function protect_workflow_run_authority()
returns trigger
language plpgsql
as $$
begin
    if old.organization_id is distinct from new.organization_id
        or old.project_id is distinct from new.project_id
        or old.id is distinct from new.id
        or old.workflow_goal_id is distinct from new.workflow_goal_id
        or old.plan_revision_id is distinct from new.plan_revision_id
        or old.plan_digest is distinct from new.plan_digest
        or old.operation_id is distinct from new.operation_id
        or old.flow_run_id is distinct from new.flow_run_id
        or old.execution_input is distinct from new.execution_input
        or old.execution_input_digest is distinct from new.execution_input_digest
        or old.requested_by is distinct from new.requested_by
        or old.requested_at is distinct from new.requested_at
    then
        raise exception 'WorkflowRun immutable authority cannot be changed';
    end if;
    return new;
end
$$;

create function protect_workflow_step_projection_identity()
returns trigger
language plpgsql
as $$
begin
    if old.organization_id is distinct from new.organization_id
        or old.project_id is distinct from new.project_id
        or old.workflow_run_id is distinct from new.workflow_run_id
        or old.step_id is distinct from new.step_id
        or old.kind is distinct from new.kind
        or old.flow_step_id is distinct from new.flow_step_id
    then
        raise exception 'WorkflowStepProjection immutable identity cannot be changed';
    end if;
    return new;
end
$$;

create trigger workflow_runs_authority_immutable
before update on workflow_runs
for each row execute function protect_workflow_run_authority();

create trigger workflow_runs_delete_rejected
before delete on workflow_runs
for each row execute function reject_workflow_immutable_mutation();

create trigger workflow_step_projections_identity_immutable
before update on workflow_step_projections
for each row execute function protect_workflow_step_projection_identity();

create trigger workflow_step_projections_delete_rejected
before delete on workflow_step_projections
for each row execute function reject_workflow_immutable_mutation();

comment on table workflow_runs is
    'Cloud semantic WorkflowRun state pinned to one exact PlanRevision, Operation, and A3S Flow run';

comment on table workflow_step_projections is
    'Rebuildable current semantic step state projected from the correlated A3S Flow history';
