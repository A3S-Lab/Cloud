alter table workflow_runs
    drop constraint workflow_runs_execution_input_check;

alter table workflow_runs
    add constraint workflow_runs_execution_input_check check (
        octet_length(execution_input) between 1 and 33554432
    );
