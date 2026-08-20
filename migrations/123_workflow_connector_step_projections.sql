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
            'service',
            'output'
        )
    );

comment on constraint workflow_step_projections_kind_check
    on workflow_step_projections is
    'WorkflowRun projection kinds admitted by the current Flow runtime; Service is limited to an exact ConnectorRevision binding by immutable WorkflowRun plan validation';

alter table workflow_step_projections
    drop constraint workflow_step_projections_selected_handle_routing_check;

alter table workflow_step_projections
    add constraint workflow_step_projections_selected_handle_routing_check check (
        selected_handle is null
        or kind = 'branch'
        or (
            kind in ('execution', 'service')
            and status = 'failed'
        )
    );

comment on constraint workflow_step_projections_selected_handle_routing_check
    on workflow_step_projections is
    'A non-Branch handle records only a failed descriptor-bound Execution or Connector route; the immutable WorkflowRun plan validates the exact handle and capability';
