alter table workflow_step_projections
    drop constraint workflow_step_projections_selected_handle_routing_check;

alter table workflow_step_projections
    add constraint workflow_step_projections_selected_handle_routing_check check (
        selected_handle is null
        or kind = 'branch'
        or (
            kind in ('transform', 'execution', 'service', 'output')
            and status = 'failed'
        )
    );

comment on constraint workflow_step_projections_selected_handle_routing_check
    on workflow_step_projections is
    'A non-Branch handle records only a failed descriptor-bound Transform, Execution, Connector, Application variable, or Application Answer route; the immutable WorkflowRun plan validates the exact handle and descriptor';
