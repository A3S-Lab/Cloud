alter table workflow_step_projections
    drop constraint workflow_step_projections_selected_handle_routing_check;

alter table workflow_step_projections
    add constraint workflow_step_projections_selected_handle_routing_check check (
        selected_handle is null
        or kind = 'branch'
        or (
            kind in ('transform', 'execution', 'agent', 'service', 'output', 'subworkflow')
            and status = 'failed'
        )
    );

comment on constraint workflow_step_projections_selected_handle_routing_check
    on workflow_step_projections is
    'A non-Branch handle records only a failed descriptor-bound Transform, Execution, Agent, Connector, Application, Output, or composite-region route; the immutable WorkflowRun plan validates the exact handle and descriptor';
