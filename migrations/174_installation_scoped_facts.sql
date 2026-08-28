create table cloud_installations (
    singleton_key boolean primary key default true check (singleton_key),
    id uuid not null unique check (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    schema_version bigint not null check (schema_version = 1),
    created_at timestamptz not null
);

insert into cloud_installations (singleton_key, id, schema_version, created_at)
values (true, gen_random_uuid(), 1, clock_timestamp());

create function current_cloud_installation_id()
returns uuid
language sql
stable
as $$
    select installation.id
      from cloud_installations installation
     where installation.singleton_key
$$;

create function reject_cloud_installation_identity_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Cloud Installation identity is immutable'
        using errcode = '23514';
end
$$;

create trigger cloud_installations_immutable
before update or delete on cloud_installations
for each row execute function reject_cloud_installation_identity_mutation();

alter table organizations
    add column installation_id uuid not null default current_cloud_installation_id(),
    add constraint organizations_installation_fk
        foreign key (installation_id) references cloud_installations (id),
    add constraint organizations_installation_identity_unique
        unique (installation_id, id);

alter table outbox_events
    add column installation_id uuid,
    add column scope_kind text,
    add column project_id uuid,
    add column environment_id uuid;

update outbox_events event
   set installation_id = organization.installation_id,
       scope_kind = 'organization'
  from organizations organization
 where organization.id = event.organization_id;

alter table audit_records
    add column installation_id uuid,
    add column scope_kind text;

update audit_records audit
   set installation_id = organization.installation_id,
       scope_kind = case
           when audit.environment_id is not null then 'environment'
           when audit.project_id is not null then 'project'
           else 'organization'
       end
  from organizations organization
 where organization.id = audit.organization_id;

alter table outbox_events
    alter column installation_id set not null,
    alter column installation_id set default current_cloud_installation_id(),
    alter column scope_kind set not null,
    alter column scope_kind set default 'organization',
    alter column organization_id drop not null,
    add constraint outbox_events_scope_shape check (
        installation_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and (
            scope_kind = 'installation'
            and organization_id is null
            and project_id is null
            and environment_id is null
            or scope_kind = 'organization'
            and organization_id is not null
            and organization_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and project_id is null
            and environment_id is null
            or scope_kind = 'project'
            and organization_id is not null
            and organization_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and project_id is not null
            and project_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and environment_id is null
            or scope_kind = 'environment'
            and organization_id is not null
            and organization_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and project_id is not null
            and project_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and environment_id is not null
            and environment_id <> '00000000-0000-0000-0000-000000000000'::uuid
        )
    ),
    add constraint outbox_events_installation_fk
        foreign key (installation_id) references cloud_installations (id),
    add constraint outbox_events_organization_scope_fk
        foreign key (installation_id, organization_id)
        references organizations (installation_id, id),
    add constraint outbox_events_project_scope_fk
        foreign key (organization_id, project_id)
        references projects (organization_id, id),
    add constraint outbox_events_environment_scope_fk
        foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id);

alter table audit_records
    alter column installation_id set not null,
    alter column installation_id set default current_cloud_installation_id(),
    alter column scope_kind set not null,
    alter column scope_kind set default 'organization',
    alter column organization_id drop not null,
    add constraint audit_records_scope_shape check (
        installation_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and (
            scope_kind = 'installation'
            and organization_id is null
            and project_id is null
            and environment_id is null
            or scope_kind = 'organization'
            and organization_id is not null
            and organization_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and project_id is null
            and environment_id is null
            or scope_kind = 'project'
            and organization_id is not null
            and organization_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and project_id is not null
            and project_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and environment_id is null
            or scope_kind = 'environment'
            and organization_id is not null
            and organization_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and project_id is not null
            and project_id <> '00000000-0000-0000-0000-000000000000'::uuid
            and environment_id is not null
            and environment_id <> '00000000-0000-0000-0000-000000000000'::uuid
        )
    ),
    add constraint audit_records_scope_attribution_shape check (
        scope_kind in ('installation', 'organization')
        and attribution_status in ('legacy_unknown', 'not_applicable')
        or scope_kind in ('project', 'environment')
        and attribution_status in ('profile_missing', 'profile_bound')
    ),
    add constraint audit_records_installation_fk
        foreign key (installation_id) references cloud_installations (id),
    add constraint audit_records_organization_scope_fk
        foreign key (installation_id, organization_id)
        references organizations (installation_id, id);

create function cloud_scope_document(
    scope_kind_value text,
    installation_id_value uuid,
    organization_id_value uuid,
    project_id_value uuid,
    environment_id_value uuid
)
returns jsonb
language sql
immutable
parallel safe
as $$
    select jsonb_strip_nulls(jsonb_build_object(
        'kind', scope_kind_value,
        'installation_id', installation_id_value,
        'organization_id', organization_id_value,
        'project_id', project_id_value,
        'environment_id', environment_id_value
    ))
$$;

create function reject_cloud_fact_scope_mutation()
returns trigger
language plpgsql
as $$
begin
    if new.installation_id is distinct from old.installation_id
       or new.scope_kind is distinct from old.scope_kind
       or new.organization_id is distinct from old.organization_id
       or new.project_id is distinct from old.project_id
       or new.environment_id is distinct from old.environment_id then
        raise exception 'Cloud fact scope is immutable'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger outbox_events_scope_immutable
before update of installation_id, scope_kind, organization_id, project_id, environment_id
on outbox_events
for each row execute function reject_cloud_fact_scope_mutation();

create trigger audit_records_scope_immutable
before update of installation_id, scope_kind, organization_id, project_id, environment_id
on audit_records
for each row execute function reject_cloud_fact_scope_mutation();

drop trigger audit_records_attribution_immutable on audit_records;
drop function reject_audit_attribution_mutation();

create function reject_audit_attribution_mutation()
returns trigger
language plpgsql
as $$
begin
    if new.attribution_profile_id is distinct from old.attribution_profile_id
       or new.attribution_status is distinct from old.attribution_status then
        raise exception 'audit attribution snapshots are immutable'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger audit_records_attribution_immutable
before update of attribution_profile_id, attribution_status
on audit_records
for each row execute function reject_audit_attribution_mutation();

create or replace function reject_audit_record_before_retention_boundary()
returns trigger
language plpgsql
as $$
declare
    retained_from timestamptz;
begin
    if new.scope_kind = 'installation' then
        return new;
    end if;
    select state.records_available_from
      into retained_from
      from audit_retention_states state
     where state.organization_id = new.organization_id
       for share;
    if not found then
        raise exception 'audit record organization has no retention authority'
            using errcode = '23503';
    end if;
    if retained_from is not null and new.occurred_at < retained_from then
        raise exception 'audit record is older than the retained availability boundary'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create index outbox_events_installation_scope_time_idx
    on outbox_events (installation_id, scope_kind, occurred_at, event_id);

create index audit_records_installation_scope_time_idx
    on audit_records (installation_id, scope_kind, occurred_at desc, audit_id desc);

comment on table cloud_installations is
    'Canonical immutable identity of this independently deployed A3S Cloud control plane';
comment on column organizations.installation_id is
    'Canonical owning Cloud Installation; assigned by the database, never supplied by a tenant';
comment on column outbox_events.scope_kind is
    'Closed exact Cloud fact scope; the Organization default permits bounded mixed-version rollout while new writers persist resolved scope explicitly';
comment on column audit_records.scope_kind is
    'Closed exact Cloud fact scope shared with Outbox; installation audit is retained indefinitely';
