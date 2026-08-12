alter table workflow_runs
    add constraint workflow_runs_execution_authority_unique unique (
        organization_id,
        project_id,
        id,
        plan_revision_id,
        plan_digest
    );

alter table executions
    add column workflow_run_id uuid,
    add column workflow_plan_revision_id uuid,
    add column workflow_plan_digest text,
    add column workflow_step_id text,
    add column workflow_step_attempt bigint,
    add column execution_template_id uuid,
    add column execution_template_revision_id uuid,
    add column execution_template_definition_digest text;

alter table executions
    add constraint executions_workflow_binding_check check (
        (
            workflow_run_id is null
            and workflow_plan_revision_id is null
            and workflow_plan_digest is null
            and workflow_step_id is null
            and workflow_step_attempt is null
            and execution_template_id is null
            and execution_template_revision_id is null
            and execution_template_definition_digest is null
        )
        or
        (
            workflow_run_id is not null
            and workflow_plan_revision_id is not null
            and workflow_plan_digest ~ '^sha256:[0-9a-f]{64}$'
            and char_length(workflow_step_id) between 1 and 96
            and workflow_step_id ~ '^[A-Za-z0-9_-]+$'
            and workflow_step_attempt > 0
            and execution_template_id is not null
            and execution_template_revision_id is not null
            and execution_template_definition_digest ~ '^sha256:[0-9a-f]{64}$'
        )
    ),
    add constraint executions_workflow_run_fk foreign key (
        organization_id,
        project_id,
        workflow_run_id,
        workflow_plan_revision_id,
        workflow_plan_digest
    ) references workflow_runs (
        organization_id,
        project_id,
        id,
        plan_revision_id,
        plan_digest
    ),
    add constraint executions_workflow_template_fk
        foreign key (
            organization_id,
            project_id,
            execution_template_id,
            execution_template_revision_id,
            execution_template_definition_digest
        )
        references execution_template_revisions (
            organization_id,
            project_id,
            template_id,
            revision_id,
            definition_digest
        );

create unique index executions_workflow_step_unique
    on executions (
        organization_id,
        workflow_run_id,
        workflow_step_id,
        workflow_step_attempt
    )
    where workflow_run_id is not null;

comment on column executions.workflow_run_id is
    'Exact owning WorkflowRun identity for an adopted finite-task child';

comment on column executions.workflow_step_id is
    'Exact parent-local Workflow step identity used to prevent duplicate child creation';
