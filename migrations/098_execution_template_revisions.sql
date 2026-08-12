create table execution_template_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    template_id uuid not null,
    revision_id uuid not null,
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 131072),
    definition_digest text not null
        check (definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, template_id, revision_id),
    unique (organization_id, project_id, template_id, revision_id),
    unique (
        organization_id,
        project_id,
        template_id,
        revision_id,
        definition_digest
    ),
    foreign key (organization_id, project_id)
        references projects (organization_id, id)
);

create index execution_template_revisions_project_created_idx
    on execution_template_revisions (
        organization_id,
        project_id,
        created_at desc,
        template_id desc,
        revision_id desc
    );

create function reject_execution_template_revision_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'ExecutionTemplate revisions are immutable';
end
$$;

create trigger execution_template_revisions_immutable
before update or delete on execution_template_revisions
for each row execute function reject_execution_template_revision_mutation();

comment on table execution_template_revisions is
    'Executions-owned immutable ACL-native finite Task definitions referenced by exact Workflow capabilities';
