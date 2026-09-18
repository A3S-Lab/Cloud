-- APP0.3-C1: exact Application Resource Grant scope (Rule 8).
-- Project grants must not imply Application delivery authority.

alter table resource_grants
    add column if not exists application_id uuid;

do $$
declare
    constraint_name text;
begin
    for constraint_name in
        select conname
          from pg_constraint
         where conrelid = 'resource_grants'::regclass
           and contype = 'c'
           and (
               pg_get_constraintdef(oid) like '%scope_kind in (%'
               or pg_get_constraintdef(oid) like '%scope_kind = ''project''%'
           )
    loop
        execute format(
            'alter table resource_grants drop constraint %I',
            constraint_name
        );
    end loop;
end
$$;

alter table resource_grants
    drop constraint if exists resource_grants_scope_kind_check;

alter table resource_grants
    add constraint resource_grants_scope_kind_check
        check (scope_kind in ('project', 'environment', 'application', 'node'));

alter table resource_grants
    drop constraint if exists resource_grants_scope_shape_check;

alter table resource_grants
    add constraint resource_grants_scope_shape_check check (
        (scope_kind = 'project'
            and project_id is not null
            and environment_id is null
            and application_id is null
            and node_id is null)
        or (scope_kind = 'environment'
            and project_id is not null
            and environment_id is not null
            and application_id is null
            and node_id is null)
        or (scope_kind = 'application'
            and project_id is not null
            and environment_id is null
            and application_id is not null
            and node_id is null)
        or (scope_kind = 'node'
            and project_id is null
            and environment_id is null
            and application_id is null
            and node_id is not null)
    );

alter table resource_grants
    drop constraint if exists resource_grants_application_fk;

alter table resource_grants
    add constraint resource_grants_application_fk
        foreign key (organization_id, project_id, application_id)
        references applications (organization_id, project_id, id);

create unique index if not exists resource_grants_active_application_idx
    on resource_grants (organization_id, membership_id, project_id, application_id)
    where scope_kind = 'application' and revoked_at is null;

drop index if exists resource_grants_active_scope_idx;

create index resource_grants_active_scope_idx
    on resource_grants (
        organization_id,
        scope_kind,
        project_id,
        environment_id,
        application_id,
        node_id
    )
    where revoked_at is null;
