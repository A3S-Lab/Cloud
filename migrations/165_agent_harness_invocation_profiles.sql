alter table agent_executions
    add column invocation_profile jsonb,
    add column invocation_profile_digest text,
    add constraint agent_executions_invocation_profile_complete check (
        (
            invocation_profile is null
            and invocation_profile_digest is null
        )
        or (
            invocation_profile is not null
            and invocation_profile_digest is not null
            and provider_bound_at is not null
        )
    ),
    add constraint agent_executions_invocation_profile_values check (
        invocation_profile is null
        or (
            jsonb_typeof(invocation_profile) = 'object'
            and invocation_profile ->> 'schema'
                = 'a3s.cloud.harness-invocation-profile.v1'
            and octet_length(invocation_profile::text) <= 262144
            and invocation_profile_digest ~ '^sha256:[0-9a-f]{64}$'
        )
    );

create function agent_executions_enforce_invocation_profile_immutability()
returns trigger
language plpgsql
as $$
begin
    if old.invocation_profile is null
        and new.invocation_profile is not null
        and old.provider_bound_at is not null then
        raise exception 'Agent Harness invocation profile must be bound before dispatch';
    end if;
    if old.invocation_profile is not null
        and (
            old.invocation_profile is distinct from new.invocation_profile
            or old.invocation_profile_digest is distinct from new.invocation_profile_digest
        ) then
        raise exception 'Agent Harness invocation profile is immutable';
    end if;
    return new;
end;
$$;

create trigger agent_executions_invocation_profile_immutable
before update of invocation_profile, invocation_profile_digest on agent_executions
for each row execute function agent_executions_enforce_invocation_profile_immutability();

comment on column agent_executions.invocation_profile is
    'A1.4 closed immutable Harness invocation profile; contains only exact identities, policy digests, and Secret references, never Secret material';

comment on column agent_executions.invocation_profile_digest is
    'Canonical JSON SHA-256 identity bound into every new provider run before dispatch; legacy unbound executions fail closed at redispatch';
