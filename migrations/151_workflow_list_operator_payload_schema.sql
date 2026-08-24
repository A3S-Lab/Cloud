alter table workflow_revision_payloads
    drop constraint workflow_revision_payloads_schema_check;

alter table workflow_revision_payloads
    add constraint workflow_revision_payloads_schema_check check (
        schema in (
            'cloud.workflow.configuration.list-operator.v1',
            'cloud.workflow.configuration.v1',
            'cloud.workflow.configuration.variable-aggregate.v1',
            'cloud.workflow.data-schema.v1',
            'cloud.workflow.policy.v1',
            'cloud.workflow.policy.v2',
            'cloud.workflow.policy.v3'
        )
    );

comment on constraint workflow_revision_payloads_schema_check
    on workflow_revision_payloads is
    'Closed Workflow payload schema registry; canonical ACL parsing remains the semantic authority';
