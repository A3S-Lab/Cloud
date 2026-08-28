create table platform_role_policy_revisions (
    installation_id uuid not null references cloud_installations (id),
    policy_id uuid not null check (policy_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    id uuid primary key check (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    revision_number bigint not null check (
        revision_number between 1 and 9007199254740991
    ),
    canonical_acl text not null check (
        octet_length(canonical_acl) between 1 and 65536
        and right(canonical_acl, 1) = E'\n'
    ),
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    accepted_by uuid not null references identity_principals (id),
    accepted_at timestamptz not null,
    unique (installation_id, policy_id, revision_number),
    unique (installation_id, policy_id, id, revision_number)
);

create table platform_role_policy_heads (
    installation_id uuid primary key references cloud_installations (id),
    policy_id uuid not null,
    revision_id uuid not null,
    revision_number bigint not null check (
        revision_number between 1 and 9007199254740991
    ),
    updated_at timestamptz not null,
    foreign key (installation_id, policy_id, revision_id, revision_number)
        references platform_role_policy_revisions (
            installation_id,
            policy_id,
            id,
            revision_number
        )
);

create table platform_role_bindings (
    id uuid primary key check (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    installation_id uuid not null references cloud_installations (id),
    principal_id uuid not null references identity_principals (id),
    role text not null check (
        role in (
            'platform_owner',
            'platform_admin',
            'platform_operator',
            'security_auditor'
        )
    ),
    aggregate_version bigint not null check (
        aggregate_version between 1 and 9007199254740991
    ),
    created_by uuid not null references identity_principals (id),
    updated_by uuid not null references identity_principals (id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    revoked_at timestamptz,
    unique (installation_id, id),
    check (updated_at >= created_at),
    check (revoked_at is null or revoked_at = updated_at)
);

create unique index platform_role_bindings_active_principal_unique
    on platform_role_bindings (installation_id, principal_id)
    where revoked_at is null;

create index platform_role_bindings_active_role_idx
    on platform_role_bindings (installation_id, role, created_at, id)
    where revoked_at is null;

create function validate_platform_role_policy_revision_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op <> 'INSERT' then
        raise exception 'accepted platform role policy revisions are immutable'
            using errcode = '23514';
    end if;

    perform 1
      from identity_principals principal
     where principal.id = new.accepted_by
       and principal.disabled_at is null
       for key share of principal;
    if not found then
        raise exception 'platform role policy acceptor must be an active Principal'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger platform_role_policy_revisions_transition_guard
before insert or update or delete on platform_role_policy_revisions
for each row execute function validate_platform_role_policy_revision_transition();

create function validate_platform_role_policy_head_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'platform role policy heads are not deletable'
            using errcode = '23514';
    end if;

    perform 1
      from cloud_installations installation
     where installation.id = new.installation_id
       and installation.singleton_key
       for update of installation;
    if not found then
        raise exception 'platform role policy head has no canonical Installation'
            using errcode = '23503';
    end if;

    if tg_op = 'INSERT' then
        if new.revision_number <> 1 then
            raise exception 'initial platform role policy head must select revision one'
                using errcode = '23514';
        end if;
        return new;
    end if;

    if new.installation_id is distinct from old.installation_id
       or new.policy_id is distinct from old.policy_id
       or new.revision_number <> old.revision_number + 1
       or new.revision_id is not distinct from old.revision_id
       or new.updated_at < old.updated_at then
        raise exception 'platform role policy head must advance to its exact successor'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger platform_role_policy_heads_transition_guard
before insert or update or delete on platform_role_policy_heads
for each row execute function validate_platform_role_policy_head_transition();

create function validate_platform_role_binding_transition()
returns trigger
language plpgsql
as $$
declare
    other_active_owner_count bigint;
begin
    if tg_op = 'DELETE' then
        raise exception 'platform role binding history is not deletable'
            using errcode = '23514';
    end if;

    perform 1
      from cloud_installations installation
     where installation.id = new.installation_id
       and installation.singleton_key
       for update of installation;
    if not found then
        raise exception 'platform role binding has no canonical Installation'
            using errcode = '23503';
    end if;

    if tg_op = 'INSERT' then
        perform 1
          from identity_principals principal
         where principal.id = new.principal_id
           and principal.disabled_at is null
           for key share of principal;
        if not found then
            raise exception 'platform role binding requires an active Principal'
                using errcode = '23514';
        end if;
        if new.aggregate_version <> 1
           or new.created_by is distinct from new.updated_by
           or new.created_at is distinct from new.updated_at
           or new.revoked_at is not null then
            raise exception 'new platform role binding is not at its initial lifecycle state'
                using errcode = '23514';
        end if;
        return new;
    end if;

    if new.id is distinct from old.id
       or new.installation_id is distinct from old.installation_id
       or new.principal_id is distinct from old.principal_id
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at
       or old.revoked_at is not null
       or new.aggregate_version <> old.aggregate_version + 1
       or new.updated_at < old.updated_at
       or (
           new.revoked_at is null
           and new.role is not distinct from old.role
       )
       or (
           new.revoked_at is not null
           and (
               new.revoked_at is distinct from new.updated_at
               or new.role is distinct from old.role
           )
       ) then
        raise exception 'platform role binding transition is invalid'
            using errcode = '23514';
    end if;

    if old.role = 'platform_owner'
       and (new.role <> 'platform_owner' or new.revoked_at is not null) then
        select count(*)
          into other_active_owner_count
          from platform_role_bindings binding
          join identity_principals principal
            on principal.id = binding.principal_id
           and principal.disabled_at is null
         where binding.installation_id = old.installation_id
           and binding.id <> old.id
           and binding.role = 'platform_owner'
           and binding.revoked_at is null;
        if other_active_owner_count < 1 then
            raise exception 'the last active platform owner cannot be removed'
                using errcode = '23514';
        end if;
    end if;
    return new;
end
$$;

create trigger platform_role_bindings_transition_guard
before insert or update or delete on platform_role_bindings
for each row execute function validate_platform_role_binding_transition();

create function assert_platform_rbac_recoverable()
returns trigger
language plpgsql
as $$
declare
    target_installation_id uuid;
begin
    target_installation_id := new.installation_id;
    perform 1
      from cloud_installations installation
     where installation.id = target_installation_id
       and installation.singleton_key
       for update of installation;

    if exists (
        select 1
          from platform_role_policy_heads head
         where head.installation_id = target_installation_id
    ) and not exists (
        select 1
          from platform_role_bindings binding
          join identity_principals principal
            on principal.id = binding.principal_id
           and principal.disabled_at is null
         where binding.installation_id = target_installation_id
           and binding.role = 'platform_owner'
           and binding.revoked_at is null
    ) then
        raise exception 'platform RBAC authority must retain an active platform owner'
            using errcode = '23514';
    end if;

    if exists (
        select 1
          from platform_role_bindings binding
         where binding.installation_id = target_installation_id
    ) and not exists (
        select 1
          from platform_role_policy_heads head
         where head.installation_id = target_installation_id
    ) then
        raise exception 'platform role bindings require one current policy head'
            using errcode = '23514';
    end if;
    return null;
end
$$;

create constraint trigger platform_role_policy_heads_recoverability_guard
after insert or update on platform_role_policy_heads
deferrable initially deferred
for each row execute function assert_platform_rbac_recoverable();

create constraint trigger platform_role_bindings_recoverability_guard
after insert or update on platform_role_bindings
deferrable initially deferred
for each row execute function assert_platform_rbac_recoverable();

create function protect_last_active_platform_owner_principal()
returns trigger
language plpgsql
as $$
declare
    owner_installation_id uuid;
begin
    if old.disabled_at is not null or new.disabled_at is null then
        return new;
    end if;

    for owner_installation_id in
        select binding.installation_id
          from platform_role_bindings binding
         where binding.principal_id = old.id
           and binding.role = 'platform_owner'
           and binding.revoked_at is null
         order by binding.installation_id
    loop
        perform 1
          from cloud_installations installation
         where installation.id = owner_installation_id
         for update of installation;
        if not exists (
            select 1
              from platform_role_bindings binding
              join identity_principals principal
                on principal.id = binding.principal_id
               and principal.disabled_at is null
             where binding.installation_id = owner_installation_id
               and binding.principal_id <> old.id
               and binding.role = 'platform_owner'
               and binding.revoked_at is null
        ) then
            raise exception 'the last active platform owner Principal cannot be disabled'
                using errcode = '23514';
        end if;
    end loop;
    return new;
end
$$;

create trigger identity_principals_platform_owner_recovery_guard
before update of disabled_at on identity_principals
for each row execute function protect_last_active_platform_owner_principal();

comment on table platform_role_policy_revisions is
    'Identity-owned immutable accepted A3S ACL history for the Installation platform-role policy';
comment on table platform_role_policy_heads is
    'Single strongly consistent current platform-role policy head per Cloud Installation';
comment on table platform_role_bindings is
    'Identity-owned Installation role bindings; current state is versioned and terminal history is undeletable';
comment on function assert_platform_rbac_recoverable() is
    'Deferred database invariant: policy and bindings become visible atomically and retain at least one active owner';
