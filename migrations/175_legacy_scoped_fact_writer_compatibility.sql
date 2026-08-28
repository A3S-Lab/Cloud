create function derive_legacy_tenant_fact_scope_kind()
returns trigger
language plpgsql
as $$
begin
    if new.scope_kind is not null then
        return new;
    end if;

    if new.organization_id is null then
        raise exception 'scope_kind must be explicit for Installation facts'
            using errcode = '23514';
    end if;

    new.scope_kind := case
        when new.environment_id is not null then 'environment'
        when new.project_id is not null then 'project'
        else 'organization'
    end;
    return new;
end
$$;

create trigger outbox_events_derive_legacy_tenant_scope
before insert on outbox_events
for each row execute function derive_legacy_tenant_fact_scope_kind();

create trigger audit_records_derive_legacy_tenant_scope
before insert on audit_records
for each row execute function derive_legacy_tenant_fact_scope_kind();

alter table outbox_events
    alter column scope_kind drop default;

alter table audit_records
    alter column scope_kind drop default;

comment on function derive_legacy_tenant_fact_scope_kind() is
    'One bounded rolling-upgrade seam for pre-174 tenant Outbox and Audit writers; Installation writers must always declare their scope';
comment on column outbox_events.scope_kind is
    'Closed exact Cloud fact scope; current writers persist it explicitly and pre-174 tenant writers derive it from canonical lineage';
comment on column audit_records.scope_kind is
    'Closed exact Cloud fact scope shared with Outbox; current writers persist it explicitly and pre-174 tenant writers derive it from canonical lineage';
