alter table workflow_step_projections
    drop constraint workflow_step_projections_kind_check;

alter table workflow_step_projections
    add constraint workflow_step_projections_kind_check check (
        kind in (
            'input',
            'transform',
            'branch',
            'human_decision',
            'execution',
            'agent',
            'service',
            'subworkflow',
            'output'
        )
    );

comment on constraint workflow_step_projections_kind_check
    on workflow_step_projections is
    'WorkflowRun projection kinds admitted by the current Flow runtime; Agent requires one exact Assets-owned AgentRelease and Agents-owned execution port';
