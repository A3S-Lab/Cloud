create table application_invocation_workflow_authorities (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    invocation_id uuid not null,
    ontology_id uuid not null,
    ontology_revision_id uuid not null,
    ontology_digest text not null
        check (ontology_digest ~ '^sha256:[0-9a-f]{64}$'),
    environment_id uuid,
    requested_by uuid not null references identity_principals(id),
    timeout_seconds bigint not null check (timeout_seconds > 0),
    primary key (organization_id, application_id, invocation_id),
    unique (organization_id, project_id, application_id, invocation_id),
    foreign key (
        organization_id,
        project_id,
        application_id,
        session_id,
        application_release_id,
        application_release_digest,
        invocation_id
    ) references application_invocations (
        organization_id,
        project_id,
        application_id,
        session_id,
        application_release_id,
        application_release_digest,
        id
    ),
    foreign key (organization_id, project_id, ontology_id)
        references ontologies (organization_id, project_id, id),
    foreign key (organization_id, ontology_id, ontology_revision_id)
        references ontology_revisions (organization_id, ontology_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id)
);

create function validate_application_invocation_workflow_authority()
returns trigger
language plpgsql
as $$
begin
    perform 1
      from application_invocations invocation
      join ontology_revisions ontology_revision
        on ontology_revision.organization_id = new.organization_id
       and ontology_revision.project_id = new.project_id
       and ontology_revision.ontology_id = new.ontology_id
       and ontology_revision.id = new.ontology_revision_id
       and ontology_revision.content_digest = new.ontology_digest
     where invocation.organization_id = new.organization_id
       and invocation.project_id = new.project_id
       and invocation.application_id = new.application_id
       and invocation.application_release_id = new.application_release_id
       and invocation.application_release_digest = new.application_release_digest
       and invocation.session_id = new.session_id
       and invocation.id = new.invocation_id;
    if not found then
        raise exception 'Application invocation Workflow authority drifted from its invocation or Ontology revision';
    end if;
    return null;
end
$$;

create trigger application_invocation_workflow_authorities_immutable
before update or delete on application_invocation_workflow_authorities
for each row execute function reject_application_session_child_mutation();

create constraint trigger application_invocation_workflow_authorities_validate
after insert on application_invocation_workflow_authorities
deferrable initially deferred
for each row execute function validate_application_invocation_workflow_authority();

comment on table application_invocation_workflow_authorities is
    'Immutable external revision and caller authority retained for restart-safe composition of one Application invocation into its ordinary WorkflowRun';
