create table tenant_support_grant_intents (
    id uuid primary key check (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    installation_id uuid not null references cloud_installations (id),
    scope_kind text not null check (scope_kind in ('organization', 'project', 'environment')),
    organization_id uuid not null,
    project_id uuid,
    environment_id uuid,
    principal_id uuid not null references identity_principals (id),
    canonical_acl text not null check (
        octet_length(canonical_acl) between 1 and 65536
        and right(canonical_acl, 1) = E'\n'
    ),
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    required_approval_count smallint not null check (required_approval_count in (1, 2)),
    requested_by uuid not null references identity_principals (id),
    authentication_id text not null check (
        char_length(authentication_id) between 1 and 1024
        and authentication_id !~ E'[\\r\\n\\x00]'
    ),
    authentication_digest text not null check (
        authentication_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    requested_at timestamptz not null,
    starts_at timestamptz not null,
    expires_at timestamptz not null,
    check (requested_at < expires_at),
    check (starts_at < expires_at),
    check (
        scope_kind = 'organization'
        and project_id is null
        and environment_id is null
        or scope_kind = 'project'
        and project_id is not null
        and project_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and environment_id is null
        or scope_kind = 'environment'
        and project_id is not null
        and project_id <> '00000000-0000-0000-0000-000000000000'::uuid
        and environment_id is not null
        and environment_id <> '00000000-0000-0000-0000-000000000000'::uuid
    )
);

create table tenant_support_grant_required_approvers (
    grant_id uuid not null references tenant_support_grant_intents (id),
    approver_id uuid not null references identity_principals (id),
    primary key (grant_id, approver_id)
);

create table tenant_support_grant_approvals (
    grant_id uuid not null,
    approver_id uuid not null,
    contract_digest text not null check (contract_digest ~ '^sha256:[0-9a-f]{64}$'),
    authentication_id text not null check (
        char_length(authentication_id) between 1 and 1024
        and authentication_id !~ E'[\\r\\n\\x00]'
    ),
    authentication_digest text not null check (
        authentication_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    policy_revision_id uuid not null references platform_role_policy_revisions (id),
    policy_digest text not null check (policy_digest ~ '^sha256:[0-9a-f]{64}$'),
    binding_id uuid not null references platform_role_bindings (id),
    binding_version bigint not null check (
        binding_version between 1 and 9007199254740991
    ),
    approved_at timestamptz not null,
    evidence_digest text not null check (evidence_digest ~ '^sha256:[0-9a-f]{64}$'),
    primary key (grant_id, approver_id),
    foreign key (grant_id, approver_id)
        references tenant_support_grant_required_approvers (grant_id, approver_id)
);

create table tenant_support_grants (
    id uuid primary key references tenant_support_grant_intents (id),
    aggregate_version bigint not null check (aggregate_version in (1, 2)),
    revocation_generation bigint not null check (revocation_generation in (0, 1)),
    accepted_at timestamptz not null,
    revoked_at timestamptz,
    revoked_by uuid references identity_principals (id),
    check ((revoked_at is null) = (revoked_by is null)),
    check (
        revoked_at is null
        and aggregate_version = 1
        and revocation_generation = 0
        or revoked_at is not null
        and aggregate_version = 2
        and revocation_generation = 1
        and revoked_at >= accepted_at
    )
);

create index tenant_support_grant_intents_scope_subject_idx
    on tenant_support_grant_intents (
        installation_id,
        organization_id,
        project_id,
        environment_id,
        principal_id,
        expires_at,
        id
    );

create index tenant_support_grants_active_idx
    on tenant_support_grants (accepted_at, id)
    where revoked_at is null;

create trigger tenant_support_grant_intents_validate_scope_lineage
before insert on tenant_support_grant_intents
for each row execute function validate_cloud_fact_scope_lineage_at_insert();

create function reject_tenant_support_grant_immutable_history_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'tenant support grant intent and approval history is immutable'
        using errcode = '23514';
end
$$;

create trigger tenant_support_grant_intents_immutable
before update or delete on tenant_support_grant_intents
for each row execute function reject_tenant_support_grant_immutable_history_mutation();

create trigger tenant_support_grant_required_approvers_immutable
before update or delete on tenant_support_grant_required_approvers
for each row execute function reject_tenant_support_grant_immutable_history_mutation();

create trigger tenant_support_grant_approvals_immutable
before update or delete on tenant_support_grant_approvals
for each row execute function reject_tenant_support_grant_immutable_history_mutation();

create function validate_tenant_support_required_approver_insert()
returns trigger
language plpgsql
as $$
declare
    intent tenant_support_grant_intents%rowtype;
begin
    select *
      into intent
      from tenant_support_grant_intents
     where id = new.grant_id
       for share;
    if not found then
        raise exception 'tenant support required approver has no immutable intent'
            using errcode = '23503';
    end if;
    if new.approver_id = intent.principal_id
       or new.approver_id = intent.requested_by then
        raise exception 'tenant support subject and requester cannot approve the grant'
            using errcode = '23514';
    end if;
    perform 1
      from identity_principals principal
     where principal.id = new.approver_id
       and principal.kind = 'human'
       and principal.disabled_at is null
       for key share of principal;
    if not found then
        raise exception 'tenant support approver must be an active human Principal'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger tenant_support_grant_required_approvers_insert_guard
before insert on tenant_support_grant_required_approvers
for each row execute function validate_tenant_support_required_approver_insert();

create function assert_tenant_support_required_approver_count()
returns trigger
language plpgsql
as $$
declare
    target_grant_id uuid;
    required_count smallint;
    actual_count bigint;
begin
    if tg_table_name = 'tenant_support_grant_intents' then
        target_grant_id := new.id;
    else
        target_grant_id := new.grant_id;
    end if;
    select intent.required_approval_count
      into required_count
      from tenant_support_grant_intents intent
     where intent.id = target_grant_id;
    if not found then
        return null;
    end if;
    select count(*)
      into actual_count
      from tenant_support_grant_required_approvers required
     where required.grant_id = target_grant_id;
    if actual_count <> required_count then
        raise exception 'tenant support required approver set does not match its ACL intent'
            using errcode = '23514';
    end if;
    return null;
end
$$;

create constraint trigger tenant_support_grant_intents_approver_count_guard
after insert on tenant_support_grant_intents
deferrable initially deferred
for each row execute function assert_tenant_support_required_approver_count();

create constraint trigger tenant_support_grant_required_approvers_count_guard
after insert on tenant_support_grant_required_approvers
deferrable initially deferred
for each row execute function assert_tenant_support_required_approver_count();

create function validate_tenant_support_approval_insert()
returns trigger
language plpgsql
as $$
declare
    intent tenant_support_grant_intents%rowtype;
begin
    select *
      into intent
      from tenant_support_grant_intents
     where id = new.grant_id
       for share;
    if not found then
        raise exception 'tenant support approval has no immutable intent'
            using errcode = '23503';
    end if;

    perform 1
      from cloud_installations installation
     where installation.id = intent.installation_id
       and installation.singleton_key
       for update of installation;
    if new.contract_digest <> intent.digest
       or new.approved_at < intent.requested_at
       or new.approved_at >= intent.expires_at then
        raise exception 'tenant support approval does not match the active intent window'
            using errcode = '23514';
    end if;

    perform 1
      from identity_principals principal
      join platform_role_bindings binding
        on binding.principal_id = principal.id
       and binding.installation_id = intent.installation_id
       and binding.id = new.binding_id
       and binding.aggregate_version = new.binding_version
       and binding.revoked_at is null
       and binding.role in ('platform_owner', 'platform_admin')
      join platform_role_policy_heads head
        on head.installation_id = binding.installation_id
       and head.revision_id = new.policy_revision_id
      join platform_role_policy_revisions revision
        on revision.id = head.revision_id
       and revision.digest = new.policy_digest
     where principal.id = new.approver_id
       and principal.kind = 'human'
       and principal.disabled_at is null
       for key share of principal, binding, head, revision;
    if not found then
        raise exception 'tenant support approval lacks current human policy and binding evidence'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger tenant_support_grant_approvals_insert_guard
before insert on tenant_support_grant_approvals
for each row execute function validate_tenant_support_approval_insert();

create function validate_tenant_support_grant_transition()
returns trigger
language plpgsql
as $$
declare
    intent tenant_support_grant_intents%rowtype;
begin
    if tg_op = 'DELETE' then
        raise exception 'accepted tenant support grant history is not deletable'
            using errcode = '23514';
    end if;

    select *
      into intent
      from tenant_support_grant_intents
     where id = new.id
       for share;
    if not found then
        raise exception 'accepted tenant support grant has no immutable intent'
            using errcode = '23503';
    end if;
    perform 1
      from cloud_installations installation
     where installation.id = intent.installation_id
       and installation.singleton_key
       for update of installation;

    if tg_op = 'INSERT' then
        if new.aggregate_version <> 1
           or new.revocation_generation <> 0
           or new.revoked_at is not null
           or new.revoked_by is not null
           or new.accepted_at < intent.requested_at
           or new.accepted_at >= intent.expires_at
           or new.accepted_at < (
               select max(approval.approved_at)
                 from tenant_support_grant_approvals approval
                where approval.grant_id = new.id
           )
           or exists (
               select 1
                 from tenant_support_grant_required_approvers required
                 left join tenant_support_grant_approvals approval
                   on approval.grant_id = required.grant_id
                  and approval.approver_id = required.approver_id
                 left join identity_principals approver
                   on approver.id = required.approver_id
                  and approver.kind = 'human'
                  and approver.disabled_at is null
                where required.grant_id = new.id
                  and (
                      approval.grant_id is null
                      or approver.id is null
                      or not exists (
                          select 1
                            from platform_role_bindings current_binding
                           where current_binding.installation_id = intent.installation_id
                             and current_binding.principal_id = required.approver_id
                             and current_binding.role in ('platform_owner', 'platform_admin')
                             and current_binding.revoked_at is null
                      )
                  )
           )
           or not exists (
               select 1
                 from identity_principals subject
                where subject.id = intent.principal_id
                  and subject.kind = 'human'
                  and subject.disabled_at is null
           ) then
            raise exception 'tenant support grant lacks complete live approval evidence'
                using errcode = '23514';
        end if;
        return new;
    end if;

    if old.revoked_at is not null
       or new.id is distinct from old.id
       or new.accepted_at is distinct from old.accepted_at
       or new.aggregate_version <> 2
       or new.revocation_generation <> 1
       or new.revoked_at is null
       or new.revoked_by is null
       or new.revoked_at < old.accepted_at then
        raise exception 'tenant support grant revocation must be one terminal generation'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger tenant_support_grants_transition_guard
before insert or update or delete on tenant_support_grants
for each row execute function validate_tenant_support_grant_transition();

comment on table tenant_support_grant_intents is
    'Immutable canonical A3S ACL support intent; approver IDs are requirements, not approval facts';
comment on table tenant_support_grant_approvals is
    'Immutable exact human approval actions bound to authentication, current policy and binding evidence';
comment on table tenant_support_grants is
    'Accepted and terminally revoked support-grant lifecycle activated only by complete approval evidence';
