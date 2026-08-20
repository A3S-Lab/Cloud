create table applications (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    name text not null check (char_length(name) between 1 and 63),
    name_key text not null check (char_length(name_key) between 1 and 63),
    description text not null check (char_length(description) <= 4096),
    experience text not null check (
        experience in (
            'chatbot',
            'text_generator',
            'classic_agent',
            'new_agent',
            'chatflow',
            'workflow'
        )
    ),
    current_release_id uuid not null,
    current_release_number bigint not null check (current_release_number > 0),
    current_release_digest text not null
        check (current_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, id),
    unique (organization_id, project_id, name_key),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    check (current_release_number = aggregate_version),
    check (aggregate_version > 1 or created_at = updated_at),
    check (updated_at >= created_at)
);

create table application_releases (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    id uuid not null,
    release_number bigint not null check (release_number > 0),
    parent_release_id uuid,
    parent_digest text check (parent_digest ~ '^sha256:[0-9a-f]{64}$'),
    experience text not null check (
        experience in (
            'chatbot',
            'text_generator',
            'classic_agent',
            'new_agent',
            'chatflow',
            'workflow'
        )
    ),
    contract_schema text not null
        check (contract_schema = 'cloud.application.release.v1'),
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 65536),
    contract_digest text not null
        check (contract_digest ~ '^sha256:[0-9a-f]{64}$'),
    workflow_definition_id uuid not null,
    workflow_revision_id uuid not null,
    workflow_contract_digest text not null
        check (workflow_contract_digest ~ '^sha256:[0-9a-f]{64}$'),
    workflow_payload_set_digest text not null
        check (workflow_payload_set_digest ~ '^sha256:[0-9a-f]{64}$'),
    workflow_semantic_contract_set_digest text not null
        check (workflow_semantic_contract_set_digest ~ '^sha256:[0-9a-f]{64}$'),
    input_schema_digest text not null
        check (input_schema_digest ~ '^sha256:[0-9a-f]{64}$'),
    output_schema_digest text not null
        check (output_schema_digest ~ '^sha256:[0-9a-f]{64}$'),
    presentation_digest text not null
        check (presentation_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, application_id, id),
    unique (organization_id, application_id, release_number),
    unique (organization_id, project_id, application_id, id),
    unique (organization_id, application_id, id, contract_digest),
    foreign key (organization_id, project_id, application_id)
        references applications (organization_id, project_id, id),
    foreign key (organization_id, application_id, parent_release_id)
        references application_releases (organization_id, application_id, id),
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
        release_number = 1
        and parent_release_id is null
        and parent_digest is null
        or release_number > 1
        and parent_release_id is not null
        and parent_digest is not null
    )
);

alter table applications
    add constraint applications_current_release_fk
    foreign key (
        organization_id,
        project_id,
        id,
        current_release_id
    ) references application_releases (
        organization_id,
        project_id,
        application_id,
        id
    )
    deferrable initially deferred;

create index applications_project_name_idx
    on applications (organization_id, project_id, name_key, id);

create index application_releases_lineage_idx
    on application_releases (
        organization_id,
        project_id,
        application_id,
        release_number desc,
        id
    );

create index application_releases_workflow_idx
    on application_releases (
        organization_id,
        project_id,
        workflow_definition_id,
        workflow_revision_id,
        application_id
    );

create function validate_application_release_lineage()
returns trigger
language plpgsql
as $$
declare
    stored_parent_number bigint;
    stored_parent_digest text;
    stored_parent_experience text;
    stored_parent_created_at timestamptz;
begin
    if new.release_number = 1 then
        if new.parent_release_id is not null or new.parent_digest is not null then
            raise exception 'initial Application release cannot have a parent';
        end if;
        return new;
    end if;

    select release_number, contract_digest, experience, created_at
      into stored_parent_number,
           stored_parent_digest,
           stored_parent_experience,
           stored_parent_created_at
      from application_releases
     where organization_id = new.organization_id
       and project_id = new.project_id
       and application_id = new.application_id
       and id = new.parent_release_id;

    if not found
       or new.release_number <> stored_parent_number + 1
       or new.parent_digest <> stored_parent_digest
       or new.contract_digest = stored_parent_digest
       or new.experience <> stored_parent_experience
       or new.created_at < stored_parent_created_at then
        raise exception 'Application release lineage is stale, forked, or a no-op';
    end if;
    return new;
end
$$;

create trigger application_releases_validate_lineage
before insert on application_releases
for each row execute function validate_application_release_lineage();

create function validate_application_release_workflow_binding()
returns trigger
language plpgsql
as $$
declare
    stored_contract_digest text;
    stored_payload_set_digest text;
begin
    select content_digest, payload_set_digest
      into stored_contract_digest, stored_payload_set_digest
      from workflow_revisions
     where organization_id = new.organization_id
       and project_id = new.project_id
       and workflow_definition_id = new.workflow_definition_id
       and id = new.workflow_revision_id;

    if not found
       or new.workflow_contract_digest <> stored_contract_digest
       or new.workflow_payload_set_digest <> stored_payload_set_digest then
        raise exception 'Application release does not match its exact Workflow revision';
    end if;
    return new;
end
$$;

create trigger application_releases_validate_workflow_binding
before insert on application_releases
for each row execute function validate_application_release_workflow_binding();

create function reject_application_release_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Application releases are immutable';
end
$$;

create trigger application_releases_immutable
before update or delete on application_releases
for each row execute function reject_application_release_mutation();

create trigger applications_no_delete
before delete on applications
for each row execute function reject_application_release_mutation();

create function validate_application_update()
returns trigger
language plpgsql
as $$
begin
    if new.organization_id <> old.organization_id
       or new.project_id <> old.project_id
       or new.id <> old.id
       or new.name <> old.name
       or new.name_key <> old.name_key
       or new.description <> old.description
       or new.experience <> old.experience
       or new.created_by <> old.created_by
       or new.created_at <> old.created_at
       or new.aggregate_version <> old.aggregate_version + 1
       or new.current_release_number <> old.current_release_number + 1
       or new.current_release_id = old.current_release_id
       or new.current_release_digest = old.current_release_digest
       or new.updated_at < old.updated_at then
        raise exception 'Application update is not a sequential release advance';
    end if;
    return new;
end
$$;

create trigger applications_validate_update
before update on applications
for each row execute function validate_application_update();

create function validate_application_head()
returns trigger
language plpgsql
as $$
begin
    perform 1
      from application_releases release
     where release.organization_id = new.organization_id
       and release.project_id = new.project_id
       and release.application_id = new.id
       and release.id = new.current_release_id
       and release.release_number = new.current_release_number
       and release.contract_digest = new.current_release_digest
       and release.experience = new.experience
       and release.created_at = new.updated_at
       and (
           new.current_release_number > 1
           or release.created_at = new.created_at
           and release.created_by = new.created_by
       );
    if not found then
        raise exception 'Application head does not match its current release';
    end if;
    return new;
end
$$;

create constraint trigger applications_validate_head
after insert or update on applications
deferrable initially deferred
for each row execute function validate_application_head();

comment on table applications is
    'Applications-owned project-scoped heads for immutable release contracts';

comment on table application_releases is
    'Immutable canonical A3S ACL release policy and exact Workflow revision evidence; Workflow and Flow retain execution authority';

comment on constraint applications_current_release_fk on applications is
    'Exact immutable Application release fence; not a mutable Workflow head';
