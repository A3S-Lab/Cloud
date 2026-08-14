create or replace function validate_connector_secret_binding_materializable()
returns trigger
language plpgsql
as $$
begin
    perform 1
      from secrets s
      join secret_versions v
        on v.secret_id = s.id
       and v.version = new.secret_version
     where s.organization_id = new.organization_id
       and s.project_id = new.project_id
       and s.environment_id = new.environment_id
       and s.id = new.secret_id
       and s.state = 'active'
       and v.state = 'active'
       for share of s, v;
    if not found then
        raise foreign_key_violation using
            message = 'Connector Secret binding is not active in its exact environment';
    end if;
    return new;
end
$$;

comment on function validate_connector_secret_binding_materializable() is
    'Admission-only active-state row fence with foreign-key error semantics; Secrets remains lifecycle authority and execution must recheck just in time';
