alter table workflow_revisions
    drop constraint workflow_revisions_compiler_schema_version_check;

alter table workflow_revisions
    add constraint workflow_revisions_compiler_schema_version_check
    check (compiler_schema_version in (1, 2));

alter table workflow_plan_revisions
    drop constraint workflow_plan_revisions_plan_schema_check;

alter table workflow_plan_revisions
    add constraint workflow_plan_revisions_plan_schema_check
    check (plan_schema in ('cloud.workflow.plan.v1', 'cloud.workflow.plan.v2'));

alter table workflow_plan_revisions
    drop constraint workflow_plan_revisions_compiler_revision_check;

alter table workflow_plan_revisions
    add constraint workflow_plan_revisions_compiler_revision_check
    check (
        (plan_schema = 'cloud.workflow.plan.v1'
            and compiler_revision = 'cloud.workflow.plan-compiler.v1')
        or
        (plan_schema = 'cloud.workflow.plan.v2'
            and compiler_revision = 'cloud.workflow.plan-compiler.v2')
    );

create table workflow_revision_semantic_contracts (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_definition_id uuid not null,
    workflow_revision_id uuid not null,
    kind text not null check (
        kind in ('descriptor_bindings', 'descriptor_registry', 'variable_contract')
    ),
    schema text not null,
    canonical_acl text not null check (octet_length(canonical_acl) between 1 and 4194304),
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    primary key (
        organization_id,
        workflow_definition_id,
        workflow_revision_id,
        kind
    ),
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
    check (
        (kind = 'descriptor_bindings'
            and schema = 'cloud.workflow.step-descriptor-bindings.v1'
            and octet_length(canonical_acl) <= 524288)
        or
        (kind = 'descriptor_registry'
            and schema = 'cloud.workflow.step-descriptor-registry.v1')
        or
        (kind = 'variable_contract'
            and schema = 'cloud.workflow.variable-contract.v1'
            and octet_length(canonical_acl) <= 2097152)
    )
);

create function validate_workflow_revision_semantic_contract_count()
returns trigger
language plpgsql
as $$
declare
    contract_count integer;
    parent_compiler_schema_version integer;
begin
    if new.parent_revision_id is not null then
        select compiler_schema_version
        into parent_compiler_schema_version
        from workflow_revisions
        where organization_id = new.organization_id
          and workflow_definition_id = new.workflow_definition_id
          and id = new.parent_revision_id;

        if parent_compiler_schema_version > new.compiler_schema_version then
            raise exception 'Workflow revisions cannot downgrade compiler schema authority';
        end if;
    end if;

    if new.compiler_schema_version = 1 then
        return new;
    end if;

    select count(*)
    into contract_count
    from workflow_revision_semantic_contracts
    where organization_id = new.organization_id
      and workflow_definition_id = new.workflow_definition_id
      and workflow_revision_id = new.id;

    if contract_count <> 3 then
        raise exception 'Workflow compiler schema 2 requires three semantic contracts';
    end if;
    return new;
end
$$;

create constraint trigger workflow_revision_semantic_contract_count
after insert on workflow_revisions
deferrable initially deferred
for each row execute function validate_workflow_revision_semantic_contract_count();

create function validate_workflow_revision_semantic_contract_parent()
returns trigger
language plpgsql
as $$
declare
    parent_compiler_schema_version integer;
begin
    select compiler_schema_version
    into parent_compiler_schema_version
    from workflow_revisions
    where organization_id = new.organization_id
      and workflow_definition_id = new.workflow_definition_id
      and id = new.workflow_revision_id;

    if parent_compiler_schema_version <> 2 then
        raise exception 'Workflow semantic contracts require compiler schema 2';
    end if;
    return new;
end
$$;

create constraint trigger workflow_revision_semantic_contract_parent
after insert on workflow_revision_semantic_contracts
deferrable initially deferred
for each row execute function validate_workflow_revision_semantic_contract_parent();

create trigger workflow_revision_semantic_contracts_immutable
before update or delete on workflow_revision_semantic_contracts
for each row execute function reject_workflow_immutable_mutation();

comment on table workflow_revision_semantic_contracts is
    'Immutable revision-owned descriptor bindings, recoverable descriptor snapshot, and typed variable contract; not a mutable node catalog or variable store';
