alter table workflow_revision_semantic_contracts
    drop constraint workflow_revision_semantic_contracts_kind_check;

alter table workflow_revision_semantic_contracts
    add constraint workflow_revision_semantic_contracts_kind_check
    check (
        kind in (
            'descriptor_bindings',
            'descriptor_registry',
            'variable_contract',
            'variable_defaults'
        )
    );

alter table workflow_revision_semantic_contracts
    drop constraint workflow_revision_semantic_contracts_check;

alter table workflow_revision_semantic_contracts
    add constraint workflow_revision_semantic_contracts_check
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
        or
        (kind = 'variable_defaults'
            and schema = 'cloud.workflow.variable-defaults.v1'
            and octet_length(canonical_acl) <= 2097152)
    );

create or replace function validate_workflow_revision_semantic_contract_count()
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

    if contract_count not in (3, 4) then
        raise exception 'Workflow compiler schema 2 requires three semantic contracts and optional default material';
    end if;
    return new;
end
$$;

alter table workflow_runs
    drop constraint workflow_runs_execution_input_check;

alter table workflow_runs
    add constraint workflow_runs_execution_input_check check (
        octet_length(execution_input) between 1 and 37748736
    );

comment on table workflow_revision_semantic_contracts is
    'Immutable revision-owned descriptor bindings, recoverable descriptor snapshot, typed variable contract, and optional digest-bound default material; not a mutable node catalog or variable store';
