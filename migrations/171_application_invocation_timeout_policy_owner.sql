alter table application_invocation_workflow_authorities
    drop constraint application_invocation_workflow_authorities_timeout_policy;

comment on column application_invocation_workflow_authorities.timeout_seconds is
    'Exact positive timeout admitted through the Applications WorkflowRun port; Workflow alone owns its default and maximum';
