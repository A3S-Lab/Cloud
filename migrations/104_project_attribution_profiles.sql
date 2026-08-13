create function project_attribution_labels_are_valid(value jsonb)
returns boolean
language sql
immutable
strict
parallel safe
as $$
    select case
        when jsonb_typeof(value) <> 'object' then false
        when (select count(*) from jsonb_object_keys(value)) > 32 then false
        else not exists (
            select 1
            from jsonb_each(value) as label(label_key, label_value)
            where label_key !~ '^[a-z][a-z0-9._-]{0,62}$'
               or jsonb_typeof(label_value) <> 'string'
               or char_length(label_value #>> '{}') not between 1 and 255
               or (label_value #>> '{}') <> btrim(label_value #>> '{}')
               or (label_value #>> '{}') ~ '[[:cntrl:]]'
        )
    end
$$;

create table project_attribution_profiles (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    previous_profile_id uuid,
    business_owner_reference text not null check (
        char_length(business_owner_reference) between 1 and 255
        and business_owner_reference = btrim(business_owner_reference)
        and business_owner_reference !~ '[[:cntrl:]]'
    ),
    cost_attribution_code text check (
        cost_attribution_code is null
        or (
            char_length(cost_attribution_code) between 1 and 128
            and cost_attribution_code = btrim(cost_attribution_code)
            and cost_attribution_code !~ '[[:cntrl:]]'
        )
    ),
    labels jsonb not null check (project_attribution_labels_are_valid(labels)),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, project_id, id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, project_id, previous_profile_id)
        references project_attribution_profiles (organization_id, project_id, id),
    check (previous_profile_id is null or previous_profile_id <> id)
);

create index project_attribution_profiles_history_idx
    on project_attribution_profiles (organization_id, project_id, created_at desc, id desc);

create function validate_project_attribution_profile_lineage()
returns trigger
language plpgsql
as $$
declare
    current_profile_id uuid;
begin
    select current_attribution_profile_id
      into current_profile_id
      from projects
     where organization_id = new.organization_id
       and id = new.project_id
       for update;

    if not found then
        raise exception 'Project attribution profile must belong to an existing project';
    end if;
    if new.previous_profile_id is distinct from current_profile_id then
        raise exception 'Project attribution profile must extend the current profile';
    end if;
    return new;
end
$$;

create trigger project_attribution_profiles_validate_lineage
before insert on project_attribution_profiles
for each row execute function validate_project_attribution_profile_lineage();

alter table projects
    add column current_attribution_profile_id uuid;

alter table projects
    add constraint projects_current_attribution_profile_fk
    foreign key (organization_id, id, current_attribution_profile_id)
    references project_attribution_profiles (organization_id, project_id, id);

create function validate_project_attribution_pointer_transition()
returns trigger
language plpgsql
as $$
declare
    profile_previous_id uuid;
begin
    if new.current_attribution_profile_id is not distinct from old.current_attribution_profile_id then
        return new;
    end if;
    if new.current_attribution_profile_id is null
       or new.aggregate_version <> old.aggregate_version + 1 then
        raise exception 'Project attribution pointer transition is invalid';
    end if;

    select previous_profile_id
      into profile_previous_id
      from project_attribution_profiles
     where organization_id = new.organization_id
       and project_id = new.id
       and id = new.current_attribution_profile_id;

    if not found
       or profile_previous_id is distinct from old.current_attribution_profile_id then
        raise exception 'Project attribution pointer must advance one profile';
    end if;
    return new;
end
$$;

create trigger projects_validate_attribution_pointer
before update of current_attribution_profile_id on projects
for each row execute function validate_project_attribution_pointer_transition();

create function reject_project_attribution_profile_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Project attribution profiles are immutable';
end
$$;

create trigger project_attribution_profiles_immutable
before update or delete on project_attribution_profiles
for each row execute function reject_project_attribution_profile_mutation();

comment on table project_attribution_profiles is
    'Immutable non-monetary showback metadata revisions; pricing, balances, invoices, credits, and billing accounts are intentionally out of scope';
