alter table audit_records
    add column project_id uuid,
    add column environment_id uuid,
    add column attribution_profile_id uuid,
    add column attribution_status text;

update audit_records
   set attribution_status = 'legacy_unknown';

alter table audit_records
    alter column attribution_status set not null,
    add constraint audit_records_attribution_shape check (
        (
            attribution_status = 'legacy_unknown'
            and project_id is null
            and environment_id is null
            and attribution_profile_id is null
        )
        or (
            attribution_status = 'not_applicable'
            and project_id is null
            and environment_id is null
            and attribution_profile_id is null
        )
        or (
            attribution_status = 'profile_missing'
            and project_id is not null
            and attribution_profile_id is null
        )
        or (
            attribution_status = 'profile_bound'
            and project_id is not null
            and attribution_profile_id is not null
        )
    ),
    add constraint audit_records_attribution_project_fk
        foreign key (organization_id, project_id)
        references projects (organization_id, id),
    add constraint audit_records_attribution_environment_fk
        foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    add constraint audit_records_attribution_profile_fk
        foreign key (organization_id, project_id, attribution_profile_id)
        references project_attribution_profiles (organization_id, project_id, id);

create function reject_new_legacy_audit_attribution()
returns trigger
language plpgsql
as $$
begin
    if new.attribution_status = 'legacy_unknown' then
        raise exception 'new audit records require explicit attribution'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger audit_records_reject_new_legacy_attribution
before insert on audit_records
for each row execute function reject_new_legacy_audit_attribution();

create function reject_audit_attribution_mutation()
returns trigger
language plpgsql
as $$
begin
    if new.project_id is distinct from old.project_id
       or new.environment_id is distinct from old.environment_id
       or new.attribution_profile_id is distinct from old.attribution_profile_id
       or new.attribution_status is distinct from old.attribution_status then
        raise exception 'audit attribution snapshots are immutable'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger audit_records_attribution_immutable
before update of project_id, environment_id, attribution_profile_id, attribution_status
on audit_records
for each row execute function reject_audit_attribution_mutation();

create index audit_records_project_attribution_query_idx
    on audit_records (organization_id, project_id, occurred_at desc, audit_id desc)
    where project_id is not null;

create index audit_records_environment_attribution_query_idx
    on audit_records (organization_id, environment_id, occurred_at desc, audit_id desc)
    where environment_id is not null;

create index audit_records_profile_attribution_query_idx
    on audit_records (organization_id, attribution_profile_id, occurred_at desc, audit_id desc)
    where attribution_profile_id is not null;

create index audit_records_attribution_status_query_idx
    on audit_records (organization_id, attribution_status, occurred_at desc, audit_id desc);

comment on column audit_records.attribution_status is
    'Closed request-time Project attribution status; legacy_unknown is migration-only and private details are never an attribution source';
comment on column audit_records.attribution_profile_id is
    'Exact immutable Project attribution profile selected no later than occurred_at; later Project pointer advances never rewrite it';
