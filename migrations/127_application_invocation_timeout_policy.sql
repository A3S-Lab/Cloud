alter table application_invocation_workflow_authorities
    add constraint application_invocation_workflow_authorities_timeout_policy
    check (timeout_seconds <= 2592000);

comment on constraint application_invocation_workflow_authorities_timeout_policy
    on application_invocation_workflow_authorities is
    'Keeps persisted Application invocation authority within the ordinary WorkflowRun 30-day admission bound';
