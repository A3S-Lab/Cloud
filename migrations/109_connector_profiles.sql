alter table secrets
    add constraint secrets_scope_id_key
    unique (organization_id, project_id, environment_id, id);

create table connector_profiles (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    id uuid not null,
    name text not null check (char_length(name) between 1 and 63),
    name_key text not null check (char_length(name_key) between 1 and 63),
    current_revision_id uuid not null,
    current_revision_number bigint not null check (current_revision_number > 0),
    current_revision_digest text not null
        check (current_revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, environment_id, id),
    unique (organization_id, project_id, environment_id, name_key),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (current_revision_number = aggregate_version),
    check (aggregate_version > 1 or created_at = updated_at),
    check (updated_at >= created_at)
);

create table connector_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    profile_id uuid not null,
    id uuid not null,
    revision_number bigint not null check (revision_number > 0),
    parent_revision_id uuid,
    parent_digest text check (parent_digest ~ '^sha256:[0-9a-f]{64}$'),
    definition_kind text not null check (definition_kind = 'http'),
    definition_schema text not null
        check (definition_schema = 'cloud.connector.http.v1'),
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 65536),
    definition_digest text not null
        check (definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    secret_binding_count bigint not null check (secret_binding_count between 0 and 2),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, profile_id, id),
    unique (organization_id, profile_id, revision_number),
    unique (organization_id, project_id, environment_id, profile_id, id),
    unique (organization_id, profile_id, id, definition_digest),
    foreign key (organization_id, project_id, environment_id, profile_id)
        references connector_profiles (organization_id, project_id, environment_id, id),
    foreign key (organization_id, profile_id, parent_revision_id)
        references connector_revisions (organization_id, profile_id, id),
    check (
        revision_number = 1
        and parent_revision_id is null
        and parent_digest is null
        or revision_number > 1
        and parent_revision_id is not null
        and parent_digest is not null
    )
);

alter table connector_profiles
    add constraint connector_profiles_current_revision_fk
    foreign key (
        organization_id,
        project_id,
        environment_id,
        id,
        current_revision_id
    )
    references connector_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        id
    )
    deferrable initially deferred;

create table connector_revision_secret_bindings (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    profile_id uuid not null,
    revision_id uuid not null,
    purpose text not null check (purpose in ('destination', 'hmac_sha256')),
    secret_id uuid not null,
    secret_version bigint not null check (secret_version > 0),
    primary key (organization_id, profile_id, revision_id, purpose),
    foreign key (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id
    ) references connector_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        id
    ),
    foreign key (organization_id, project_id, environment_id, secret_id)
        references secrets (organization_id, project_id, environment_id, id),
    foreign key (secret_id, secret_version)
        references secret_versions (secret_id, version)
);

create index connector_profiles_environment_name_idx
    on connector_profiles (
        organization_id,
        project_id,
        environment_id,
        name_key,
        id
    );

create index connector_revisions_lineage_idx
    on connector_revisions (
        organization_id,
        profile_id,
        revision_number desc,
        id
    );

create index connector_revision_secret_bindings_secret_idx
    on connector_revision_secret_bindings (
        organization_id,
        project_id,
        environment_id,
        secret_id,
        secret_version
    );

create function validate_connector_revision_lineage()
returns trigger
language plpgsql
as $$
declare
    stored_parent_number bigint;
    stored_parent_digest text;
    stored_parent_created_at timestamptz;
begin
    if new.revision_number = 1 then
        if new.parent_revision_id is not null or new.parent_digest is not null then
            raise exception 'initial Connector revision cannot have a parent';
        end if;
        return new;
    end if;

    select revision_number, definition_digest, created_at
      into stored_parent_number, stored_parent_digest, stored_parent_created_at
      from connector_revisions
     where organization_id = new.organization_id
       and project_id = new.project_id
       and environment_id = new.environment_id
       and profile_id = new.profile_id
       and id = new.parent_revision_id;

    if not found
       or new.revision_number <> stored_parent_number + 1
       or new.parent_digest <> stored_parent_digest
       or new.definition_digest = stored_parent_digest
       or new.created_at < stored_parent_created_at then
        raise exception 'Connector revision lineage is stale, forked, or a no-op';
    end if;
    return new;
end
$$;

create trigger connector_revisions_validate_lineage
before insert on connector_revisions
for each row execute function validate_connector_revision_lineage();

create function validate_connector_profile_update()
returns trigger
language plpgsql
as $$
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
       or new.current_revision_number <> old.current_revision_number + 1
       or new.updated_at < old.updated_at then
        raise exception 'Connector profile update is not a sequential revision advance';
    end if;
    return new;
end
$$;

create trigger connector_profiles_validate_update
before update on connector_profiles
for each row execute function validate_connector_profile_update();

create function validate_connector_profile_head()
returns trigger
language plpgsql
as $$
begin
    perform 1
      from connector_revisions revision
     where revision.organization_id = new.organization_id
       and revision.project_id = new.project_id
       and revision.environment_id = new.environment_id
       and revision.profile_id = new.id
       and revision.id = new.current_revision_id
       and revision.revision_number = new.current_revision_number
       and revision.definition_digest = new.current_revision_digest
       and revision.created_at = new.updated_at
       and (
           new.current_revision_number > 1
           or revision.created_at = new.created_at
           and revision.created_by = new.created_by
       );
    if not found then
        raise exception 'Connector profile head does not match its current revision';
    end if;
    return new;
end
$$;

create constraint trigger connector_profiles_validate_head
after insert or update on connector_profiles
deferrable initially deferred
for each row execute function validate_connector_profile_head();

create function validate_connector_revision_binding_count()
returns trigger
language plpgsql
as $$
declare
    stored_count bigint;
begin
    select count(*)
      into stored_count
      from connector_revision_secret_bindings
     where organization_id = new.organization_id
       and profile_id = new.profile_id
       and revision_id = new.id;
    if stored_count <> new.secret_binding_count then
        raise exception 'Connector revision Secret binding count does not match';
    end if;
    return new;
end
$$;

create constraint trigger connector_revisions_validate_binding_count
after insert on connector_revisions
deferrable initially deferred
for each row execute function validate_connector_revision_binding_count();

create function validate_connector_binding_count()
returns trigger
language plpgsql
as $$
declare
    expected_count bigint;
    stored_count bigint;
begin
    select secret_binding_count
      into expected_count
      from connector_revisions
     where organization_id = new.organization_id
       and profile_id = new.profile_id
       and id = new.revision_id;
    select count(*)
      into stored_count
      from connector_revision_secret_bindings
     where organization_id = new.organization_id
       and profile_id = new.profile_id
       and revision_id = new.revision_id;
    if expected_count is null or stored_count <> expected_count then
        raise exception 'Connector revision Secret bindings are incomplete or excessive';
    end if;
    return new;
end
$$;

create constraint trigger connector_secret_bindings_validate_count
after insert on connector_revision_secret_bindings
deferrable initially deferred
for each row execute function validate_connector_binding_count();

create function reject_connector_revision_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Connector revisions are immutable';
end
$$;

create trigger connector_revisions_immutable
before update or delete on connector_revisions
for each row execute function reject_connector_revision_mutation();

create trigger connector_secret_bindings_immutable
before update or delete on connector_revision_secret_bindings
for each row execute function reject_connector_revision_mutation();

create trigger connector_profiles_no_delete
before delete on connector_profiles
for each row execute function reject_connector_revision_mutation();

comment on table connector_profiles is
    'Connectors-owned environment-scoped heads for immutable ACL-native outbound connection revisions';

comment on table connector_revisions is
    'Immutable canonical A3S ACL Connector definitions; not an execution queue, scheduler, retry store, or Secret authority';

comment on table connector_revision_secret_bindings is
    'Exact Secret ID and version references derived from Connector ACL; never plaintext or copied Secret state';
