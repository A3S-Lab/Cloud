create table workflow_definitions (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    name text not null check (char_length(name) between 1 and 120),
    name_key text not null check (char_length(name_key) between 1 and 120),
    description text not null check (char_length(description) <= 4096),
    current_revision_id uuid not null,
    current_revision_number bigint not null check (current_revision_number > 0),
    current_revision_digest text not null
        check (current_revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, name_key),
    unique (organization_id, project_id, id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    check (current_revision_number = aggregate_version),
    check (updated_at >= created_at)
);

create table workflow_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_definition_id uuid not null,
    id uuid not null,
    revision_number bigint not null check (revision_number > 0),
    parent_revision_id uuid,
    parent_digest text check (parent_digest ~ '^sha256:[0-9a-f]{64}$'),
    contract_schema text not null check (contract_schema = 'cloud.workflow.definition.v1'),
    compiler_schema_version integer not null check (compiler_schema_version = 1),
    canonical_acl text not null check (octet_length(canonical_acl) between 1 and 1048576),
    content_digest text not null check (content_digest ~ '^sha256:[0-9a-f]{64}$'),
    payload_set_digest text not null check (payload_set_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, workflow_definition_id, id),
    unique (organization_id, workflow_definition_id, revision_number),
    unique (organization_id, project_id, workflow_definition_id, id),
    foreign key (organization_id, project_id, workflow_definition_id)
        references workflow_definitions (organization_id, project_id, id),
    foreign key (organization_id, workflow_definition_id, parent_revision_id)
        references workflow_revisions (organization_id, workflow_definition_id, id),
    check (
        (
            revision_number = 1
            and parent_revision_id is null
            and parent_digest is null
        )
        or
        (
            revision_number > 1
            and parent_revision_id is not null
            and parent_digest is not null
        )
    )
);

create table workflow_revision_payloads (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_definition_id uuid not null,
    workflow_revision_id uuid not null,
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    kind text not null check (kind in ('configuration', 'data_schema', 'policy')),
    schema text not null check (
        schema in (
            'cloud.workflow.configuration.v1',
            'cloud.workflow.data-schema.v1',
            'cloud.workflow.policy.v1'
        )
    ),
    canonical_acl text not null check (octet_length(canonical_acl) between 1 and 262144),
    primary key (organization_id, workflow_definition_id, workflow_revision_id, digest),
    foreign key (
        organization_id,
        project_id,
        workflow_definition_id,
        workflow_revision_id
    ) references workflow_revisions (
        organization_id,
        project_id,
        workflow_definition_id,
        id
    )
);

alter table workflow_definitions
    add constraint workflow_definitions_current_revision_fk
    foreign key (organization_id, id, current_revision_id)
    references workflow_revisions (organization_id, workflow_definition_id, id)
    deferrable initially deferred;

alter table ontology_revisions
    add constraint ontology_revisions_project_identity_unique
    unique (organization_id, project_id, ontology_id, id);

create table workflow_goals (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    name text not null check (char_length(name) between 1 and 120),
    contract_schema text not null check (contract_schema = 'cloud.workflow.goal.v1'),
    canonical_acl text not null check (octet_length(canonical_acl) between 1 and 262144),
    contract_digest text not null check (contract_digest ~ '^sha256:[0-9a-f]{64}$'),
    input_digest text not null check (input_digest ~ '^sha256:[0-9a-f]{64}$'),
    workflow_definition_id uuid not null,
    workflow_revision_id uuid not null,
    workflow_digest text not null check (workflow_digest ~ '^sha256:[0-9a-f]{64}$'),
    ontology_id uuid not null,
    ontology_revision_id uuid not null,
    ontology_digest text not null check (ontology_digest ~ '^sha256:[0-9a-f]{64}$'),
    environment_id uuid,
    plan_revision_id uuid not null,
    plan_digest text not null check (plan_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (
        organization_id,
        project_id,
        workflow_definition_id,
        workflow_revision_id
    ) references workflow_revisions (
        organization_id,
        project_id,
        workflow_definition_id,
        id
    ),
    foreign key (
        organization_id,
        project_id,
        ontology_id,
        ontology_revision_id
    ) references ontology_revisions (
        organization_id,
        project_id,
        ontology_id,
        id
    ),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id)
);

create table workflow_plan_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_goal_id uuid not null,
    id uuid not null,
    plan_schema text not null check (plan_schema = 'cloud.workflow.plan.v1'),
    compiler_revision text not null
        check (compiler_revision = 'cloud.workflow.plan-compiler.v1'),
    canonical_plan text not null check (octet_length(canonical_plan) between 1 and 8388608),
    plan_digest text not null check (plan_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, workflow_goal_id, id),
    unique (organization_id, project_id, workflow_goal_id, id),
    foreign key (organization_id, project_id, workflow_goal_id)
        references workflow_goals (organization_id, project_id, id)
);

alter table workflow_goals
    add constraint workflow_goals_plan_revision_fk
    foreign key (organization_id, id, plan_revision_id)
    references workflow_plan_revisions (organization_id, workflow_goal_id, id)
    deferrable initially deferred;

create index workflow_definitions_project_updated_idx
    on workflow_definitions (organization_id, project_id, updated_at desc, id);

create index workflow_revisions_lineage_idx
    on workflow_revisions (
        organization_id,
        workflow_definition_id,
        revision_number desc,
        id
    );

create index workflow_goals_project_created_idx
    on workflow_goals (organization_id, project_id, created_at desc, id);

create function reject_workflow_immutable_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Workflow immutable records cannot be changed';
end
$$;

create trigger workflow_revisions_immutable
before update or delete on workflow_revisions
for each row execute function reject_workflow_immutable_mutation();

create trigger workflow_revision_payloads_immutable
before update or delete on workflow_revision_payloads
for each row execute function reject_workflow_immutable_mutation();

create trigger workflow_goals_immutable
before update or delete on workflow_goals
for each row execute function reject_workflow_immutable_mutation();

create trigger workflow_plan_revisions_immutable
before update or delete on workflow_plan_revisions
for each row execute function reject_workflow_immutable_mutation();

update api_tokens
set scopes = scopes || '["workflow:write"]'::jsonb
where (scopes ? 'platform:write' or scopes ? 'project:write')
  and not scopes ? 'workflow:write';

comment on table workflow_definitions is
    'Project-scoped Workflow aggregate heads; immutable revisions are the semantic authority';

comment on table workflow_revision_payloads is
    'Canonical closed configuration, data-schema, and policy payloads atomically owned by one Workflow revision';

comment on table workflow_goals is
    'Immutable goal inputs bound to exact Workflow and Ontology revisions';

comment on table workflow_plan_revisions is
    'Deterministic immutable plans derived from exact semantic inputs; A3S Flow remains execution authority';
