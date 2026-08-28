alter table outbox_events
    drop constraint outbox_events_organization_scope_fk,
    drop constraint outbox_events_project_scope_fk,
    drop constraint outbox_events_environment_scope_fk;

alter table audit_records
    drop constraint audit_records_organization_scope_fk;

create function validate_cloud_fact_scope_lineage_at_insert()
returns trigger
language plpgsql
as $$
begin
    case new.scope_kind
        when 'installation' then
            return new;
        when 'organization' then
            perform 1
              from organizations tenant
             where tenant.installation_id = new.installation_id
               and tenant.id = new.organization_id
               for key share of tenant;
        when 'project' then
            perform 1
              from organizations tenant
              join projects project_row
                on project_row.organization_id = tenant.id
             where tenant.installation_id = new.installation_id
               and tenant.id = new.organization_id
               and project_row.id = new.project_id
               for key share of tenant, project_row;
        when 'environment' then
            perform 1
              from organizations tenant
              join projects project_row
                on project_row.organization_id = tenant.id
              join environments environment_row
                on environment_row.organization_id = project_row.organization_id
               and environment_row.project_id = project_row.id
             where tenant.installation_id = new.installation_id
               and tenant.id = new.organization_id
               and project_row.id = new.project_id
               and environment_row.id = new.environment_id
               for key share of tenant, project_row, environment_row;
        else
            return new;
    end case;

    if not found then
        raise exception 'Cloud fact scope does not resolve to a live canonical lineage'
            using errcode = '23503';
    end if;
    return new;
end
$$;

create trigger outbox_events_validate_scope_lineage
before insert on outbox_events
for each row execute function validate_cloud_fact_scope_lineage_at_insert();

create trigger audit_records_validate_scope_lineage
before insert on audit_records
for each row execute function validate_cloud_fact_scope_lineage_at_insert();

comment on function validate_cloud_fact_scope_lineage_at_insert() is
    'One insert-time lineage authority shared by Audit and Outbox; locked live owners validate new facts while immutable historical facts outlive tenant aggregate deletion';
comment on column outbox_events.organization_id is
    'Immutable Organization identity snapshot validated and key-share locked at insert; it is not a lifecycle foreign key';
comment on column audit_records.organization_id is
    'Immutable Organization identity snapshot validated and key-share locked at insert; retained audit history outlives tenant aggregate deletion';
