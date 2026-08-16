alter table build_runs
    add constraint build_runs_durable_cell_scope_unique
    unique (organization_id, project_id, environment_id, id);

create table durable_cell_applications (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    name text not null check (char_length(name) between 1 and 63),
    name_key text not null check (char_length(name_key) between 1 and 63),
    desired_state text not null check (desired_state in ('running', 'stopped')),
    current_revision_id uuid not null,
    current_revision_number bigint not null check (current_revision_number > 0),
    current_definition_digest text not null
        check (current_definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, environment_id, id),
    unique (organization_id, project_id, environment_id, name_key),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (aggregate_version >= current_revision_number),
    check (updated_at >= created_at),
    check (
        aggregate_version > 1
        or current_revision_number = 1
        and created_at = updated_at
    )
);

create table durable_cell_application_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    application_id uuid not null,
    id uuid not null,
    revision_number bigint not null check (revision_number > 0),
    parent_revision_id uuid,
    parent_definition_digest text
        check (parent_definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    definition_schema text not null
        check (definition_schema = 'cloud.durable-cell.application.v1'),
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 262144),
    definition_digest text not null
        check (definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    build_run_id uuid not null,
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, application_id, id),
    unique (organization_id, application_id, revision_number),
    unique (organization_id, project_id, environment_id, application_id, id),
    unique (organization_id, application_id, id, definition_digest),
    foreign key (organization_id, project_id, environment_id, application_id)
        references durable_cell_applications (
            organization_id,
            project_id,
            environment_id,
            id
        ),
    foreign key (organization_id, application_id, parent_revision_id)
        references durable_cell_application_revisions (
            organization_id,
            application_id,
            id
        ),
    foreign key (organization_id, project_id, environment_id, build_run_id)
        references build_runs (organization_id, project_id, environment_id, id),
    check (
        revision_number = 1
        and parent_revision_id is null
        and parent_definition_digest is null
        or revision_number > 1
        and parent_revision_id is not null
        and parent_definition_digest is not null
    )
);

alter table durable_cell_applications
    add constraint durable_cell_applications_current_revision_fk
    foreign key (
        organization_id,
        project_id,
        environment_id,
        id,
        current_revision_id
    ) references durable_cell_application_revisions (
        organization_id,
        project_id,
        environment_id,
        application_id,
        id
    )
    deferrable initially deferred;

create index durable_cell_applications_environment_name_idx
    on durable_cell_applications (
        organization_id,
        project_id,
        environment_id,
        name_key,
        id
    );

create index durable_cell_application_revisions_lineage_idx
    on durable_cell_application_revisions (
        organization_id,
        application_id,
        revision_number desc,
        id
    );

create index durable_cell_application_revisions_build_run_idx
    on durable_cell_application_revisions (
        organization_id,
        project_id,
        environment_id,
        build_run_id,
        application_id
    );

create function validate_durable_cell_revision_lineage()
returns trigger
language plpgsql
as $$
declare
    stored_parent_number bigint;
    stored_parent_digest text;
    stored_parent_created_at timestamptz;
begin
    if new.revision_number = 1 then
        if new.parent_revision_id is not null
           or new.parent_definition_digest is not null then
            raise exception 'initial Durable Cell revision cannot have a parent';
        end if;
        return new;
    end if;

    select revision_number, definition_digest, created_at
      into stored_parent_number, stored_parent_digest, stored_parent_created_at
      from durable_cell_application_revisions
     where organization_id = new.organization_id
       and project_id = new.project_id
       and environment_id = new.environment_id
       and application_id = new.application_id
       and id = new.parent_revision_id;

    if not found
       or new.revision_number <> stored_parent_number + 1
       or new.parent_definition_digest <> stored_parent_digest
       or new.definition_digest = stored_parent_digest
       or new.created_at < stored_parent_created_at then
        raise exception 'Durable Cell revision lineage is stale, forked, or a no-op';
    end if;
    return new;
end
$$;

create trigger durable_cell_application_revisions_validate_lineage
before insert on durable_cell_application_revisions
for each row execute function validate_durable_cell_revision_lineage();

create function reject_durable_cell_revision_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Durable Cell application revisions are immutable';
end
$$;

create trigger durable_cell_application_revisions_immutable
before update or delete on durable_cell_application_revisions
for each row execute function reject_durable_cell_revision_mutation();

create function validate_durable_cell_application_update()
returns trigger
language plpgsql
as $$
declare
    stored_revision_created_at timestamptz;
begin
    if new.organization_id <> old.organization_id
       or new.project_id <> old.project_id
       or new.environment_id <> old.environment_id
       or new.id <> old.id
       or new.name <> old.name
       or new.name_key <> old.name_key
       or new.created_by <> old.created_by
       or new.created_at <> old.created_at
       or new.aggregate_version <> old.aggregate_version + 1
       or new.updated_at < old.updated_at then
        raise exception 'Durable Cell application update changed immutable or sequential state';
    end if;

    if new.current_revision_number = old.current_revision_number then
        if new.current_revision_id <> old.current_revision_id
           or new.current_definition_digest <> old.current_definition_digest
           or new.desired_state = old.desired_state then
            raise exception 'Durable Cell desired-state update changed revision authority or was a no-op';
        end if;
    elsif new.current_revision_number = old.current_revision_number + 1 then
        if new.current_revision_id = old.current_revision_id
           or new.current_definition_digest = old.current_definition_digest
           or new.desired_state <> old.desired_state then
            raise exception 'Durable Cell revision advance changed desired-state authority';
        end if;
        select created_at
          into stored_revision_created_at
          from durable_cell_application_revisions
         where organization_id = new.organization_id
           and project_id = new.project_id
           and environment_id = new.environment_id
           and application_id = new.id
           and id = new.current_revision_id
           and revision_number = new.current_revision_number
           and definition_digest = new.current_definition_digest;
        if not found or new.updated_at <> stored_revision_created_at then
            raise exception 'Durable Cell revision advance does not match its immutable revision time';
        end if;
    else
        raise exception 'Durable Cell revision number did not advance sequentially';
    end if;
    return new;
end
$$;

create trigger durable_cell_applications_validate_update
before update on durable_cell_applications
for each row execute function validate_durable_cell_application_update();

create function validate_durable_cell_application_head()
returns trigger
language plpgsql
as $$
begin
    perform 1
      from durable_cell_application_revisions revision
     where revision.organization_id = new.organization_id
       and revision.project_id = new.project_id
       and revision.environment_id = new.environment_id
       and revision.application_id = new.id
       and revision.id = new.current_revision_id
       and revision.revision_number = new.current_revision_number
       and revision.definition_digest = new.current_definition_digest
       and revision.created_at <= new.updated_at
       and (
           new.aggregate_version > 1
           or revision.revision_number = 1
           and revision.created_by = new.created_by
           and revision.created_at = new.created_at
           and revision.created_at = new.updated_at
       );
    if not found then
        raise exception 'Durable Cell application head does not match its current revision';
    end if;
    return new;
end
$$;

create constraint trigger durable_cell_applications_validate_head
after insert or update on durable_cell_applications
deferrable initially deferred
for each row execute function validate_durable_cell_application_head();

comment on table durable_cell_applications is
    'Durable Cell tenant intent only; Workloads, Operations, Gateway, and S0 retain lifecycle authority';

comment on table durable_cell_application_revisions is
    'Immutable canonical A3S ACL revisions bound to an existing tenant-scoped BuildRun';

comment on constraint durable_cell_applications_current_revision_fk
    on durable_cell_applications is
    'Exact current immutable revision fence; not a deployment pointer or provider receipt';
