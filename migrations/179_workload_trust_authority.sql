create table trust_domain_revisions (
    installation_id uuid not null references cloud_installations (id),
    trust_domain_id uuid not null check (
        trust_domain_id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    id uuid primary key check (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    revision_number bigint not null check (
        revision_number between 1 and 9007199254740991
    ),
    previous_revision_id uuid,
    name text not null check (
        char_length(name) between 1 and 253
        and name = lower(name)
        and position(chr(10) in name) = 0
        and position(chr(13) in name) = 0
    ),
    canonical_acl text not null check (
        octet_length(canonical_acl) between 1 and 32768
        and right(canonical_acl, 1) = E'\n'
    ),
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    accepted_by uuid not null references identity_principals (id),
    accepted_at timestamptz not null,
    unique (installation_id, trust_domain_id, revision_number),
    unique (installation_id, trust_domain_id, id),
    unique (installation_id, trust_domain_id, id, revision_number, name),
    foreign key (installation_id, trust_domain_id, previous_revision_id)
        references trust_domain_revisions (installation_id, trust_domain_id, id),
    check (
        revision_number = 1 and previous_revision_id is null
        or revision_number > 1
        and previous_revision_id is not null
        and previous_revision_id <> id
    )
);

create table trust_domain_heads (
    installation_id uuid not null references cloud_installations (id),
    trust_domain_id uuid not null,
    revision_id uuid not null,
    revision_number bigint not null check (
        revision_number between 1 and 9007199254740991
    ),
    name text not null,
    updated_at timestamptz not null,
    primary key (installation_id, trust_domain_id),
    unique (installation_id, name),
    foreign key (
        installation_id,
        trust_domain_id,
        revision_id,
        revision_number,
        name
    ) references trust_domain_revisions (
        installation_id,
        trust_domain_id,
        id,
        revision_number,
        name
    )
);

create function validate_trust_domain_revision_insert()
returns trigger
language plpgsql
as $$
declare
    current_head trust_domain_heads%rowtype;
    previous_revision trust_domain_revisions%rowtype;
begin
    if tg_op <> 'INSERT' then
        raise exception 'accepted trust-domain revisions are immutable'
            using errcode = '23514';
    end if;

    perform 1
      from cloud_installations installation
     where installation.id = new.installation_id
       and installation.singleton_key
       for update of installation;
    if not found then
        raise exception 'trust-domain revision has no canonical Installation'
            using errcode = '23503';
    end if;
    perform 1
      from identity_principals principal
     where principal.id = new.accepted_by
       and principal.disabled_at is null
       for key share of principal;
    if not found then
        raise exception 'trust-domain revision acceptor must be an active Principal'
            using errcode = '23514';
    end if;

    select *
      into current_head
      from trust_domain_heads head
     where head.installation_id = new.installation_id
       and head.trust_domain_id = new.trust_domain_id
       for update of head;
    if new.revision_number = 1 then
        if found or new.previous_revision_id is not null then
            raise exception 'initial trust-domain revision requires an empty head'
                using errcode = '23514';
        end if;
        return new;
    end if;
    if not found or current_head.revision_id <> new.previous_revision_id then
        raise exception 'trust-domain predecessor is not the exact current head'
            using errcode = '23514';
    end if;
    select *
      into previous_revision
      from trust_domain_revisions revision
     where revision.installation_id = new.installation_id
       and revision.trust_domain_id = new.trust_domain_id
       and revision.id = new.previous_revision_id
       for key share of revision;
    if not found
       or new.revision_number <> previous_revision.revision_number + 1
       or new.name <> previous_revision.name
       or new.accepted_at < previous_revision.accepted_at then
        raise exception 'trust-domain revision is not the exact stable-name successor'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger trust_domain_revisions_transition_guard
before insert or update or delete on trust_domain_revisions
for each row execute function validate_trust_domain_revision_insert();

create function validate_trust_domain_head_transition()
returns trigger
language plpgsql
as $$
declare
    accepted trust_domain_revisions%rowtype;
begin
    if tg_op = 'DELETE' then
        raise exception 'trust-domain heads are not deletable'
            using errcode = '23514';
    end if;
    perform 1
      from cloud_installations installation
     where installation.id = new.installation_id
       and installation.singleton_key
       for update of installation;
    if not found then
        raise exception 'trust-domain head has no canonical Installation'
            using errcode = '23503';
    end if;
    select *
      into accepted
      from trust_domain_revisions revision
     where revision.installation_id = new.installation_id
       and revision.trust_domain_id = new.trust_domain_id
       and revision.id = new.revision_id
       and revision.revision_number = new.revision_number
       and revision.name = new.name
       for key share of revision;
    if not found then
        raise exception 'trust-domain head has no exact accepted revision'
            using errcode = '23503';
    end if;
    if tg_op = 'INSERT' then
        if new.revision_number <> 1 or accepted.previous_revision_id is not null then
            raise exception 'initial trust-domain head must select revision one'
                using errcode = '23514';
        end if;
        return new;
    end if;
    if new.installation_id is distinct from old.installation_id
       or new.trust_domain_id is distinct from old.trust_domain_id
       or new.name is distinct from old.name
       or new.revision_number <> old.revision_number + 1
       or accepted.previous_revision_id is distinct from old.revision_id
       or new.updated_at < old.updated_at then
        raise exception 'trust-domain head must advance to its exact successor'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger trust_domain_heads_transition_guard
before insert or update or delete on trust_domain_heads
for each row execute function validate_trust_domain_head_transition();

create table workload_identity_policy_revisions (
    installation_id uuid not null,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    policy_id uuid not null check (
        policy_id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    id uuid primary key check (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    revision_number bigint not null check (
        revision_number between 1 and 9007199254740991
    ),
    previous_revision_id uuid,
    trust_domain_id uuid not null,
    trust_domain_revision_id uuid not null,
    workload_id uuid not null,
    workload_revision_id uuid not null,
    node_pool_id uuid not null,
    canonical_acl text not null check (
        octet_length(canonical_acl) between 1 and 65536
        and right(canonical_acl, 1) = E'\n'
    ),
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    accepted_by uuid not null references identity_principals (id),
    accepted_at timestamptz not null,
    unique (organization_id, policy_id, revision_number),
    unique (organization_id, policy_id, id),
    unique (
        installation_id,
        organization_id,
        project_id,
        environment_id,
        policy_id,
        workload_id,
        id,
        revision_number,
        trust_domain_id,
        trust_domain_revision_id
    ),
    foreign key (installation_id, organization_id)
        references organizations (installation_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, project_id, environment_id, workload_id)
        references workloads (organization_id, project_id, environment_id, id),
    foreign key (workload_id, workload_revision_id)
        references workload_revisions (workload_id, id),
    foreign key (organization_id, node_pool_id)
        references node_pools (organization_id, id),
    foreign key (installation_id, trust_domain_id, trust_domain_revision_id)
        references trust_domain_revisions (installation_id, trust_domain_id, id),
    foreign key (organization_id, policy_id, previous_revision_id)
        references workload_identity_policy_revisions (organization_id, policy_id, id),
    check (
        revision_number = 1 and previous_revision_id is null
        or revision_number > 1
        and previous_revision_id is not null
        and previous_revision_id <> id
    )
);

create table workload_identity_policy_heads (
    installation_id uuid not null,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    policy_id uuid not null,
    workload_id uuid not null,
    revision_id uuid not null,
    revision_number bigint not null check (
        revision_number between 1 and 9007199254740991
    ),
    trust_domain_id uuid not null,
    trust_domain_revision_id uuid not null,
    updated_at timestamptz not null,
    primary key (organization_id, policy_id),
    unique (policy_id),
    unique (organization_id, workload_id),
    foreign key (installation_id, organization_id)
        references organizations (installation_id, id),
    foreign key (
        installation_id,
        organization_id,
        project_id,
        environment_id,
        policy_id,
        workload_id,
        revision_id,
        revision_number,
        trust_domain_id,
        trust_domain_revision_id
    ) references workload_identity_policy_revisions (
        installation_id,
        organization_id,
        project_id,
        environment_id,
        policy_id,
        workload_id,
        id,
        revision_number,
        trust_domain_id,
        trust_domain_revision_id
    )
);

create function validate_workload_identity_policy_revision_insert()
returns trigger
language plpgsql
as $$
declare
    current_head workload_identity_policy_heads%rowtype;
    previous_revision workload_identity_policy_revisions%rowtype;
    trust_revision trust_domain_revisions%rowtype;
begin
    if tg_op <> 'INSERT' then
        raise exception 'accepted workload identity policy revisions are immutable'
            using errcode = '23514';
    end if;
    perform 1
      from cloud_installations installation
     where installation.id = new.installation_id
       and installation.singleton_key
       for update of installation;
    if not found then
        raise exception 'workload identity policy has no canonical Installation'
            using errcode = '23503';
    end if;
    perform 1
      from identity_principals principal
     where principal.id = new.accepted_by
       and principal.disabled_at is null
       for key share of principal;
    if not found then
        raise exception 'workload identity policy acceptor must be an active Principal'
            using errcode = '23514';
    end if;
    select revision.*
      into trust_revision
      from trust_domain_revisions revision
      join trust_domain_heads head
        on head.installation_id = revision.installation_id
       and head.trust_domain_id = revision.trust_domain_id
       and head.revision_id = revision.id
     where revision.installation_id = new.installation_id
       and revision.trust_domain_id = new.trust_domain_id
       and revision.id = new.trust_domain_revision_id
       for key share of revision, head;
    if not found or new.accepted_at < trust_revision.accepted_at then
        raise exception 'workload identity policy must bind the exact current trust-domain revision'
            using errcode = '23514';
    end if;
    select *
      into current_head
      from workload_identity_policy_heads head
     where head.organization_id = new.organization_id
       and head.policy_id = new.policy_id
       for update of head;
    if new.revision_number = 1 then
        if found or new.previous_revision_id is not null then
            raise exception 'initial workload identity policy revision requires an empty head'
                using errcode = '23514';
        end if;
        return new;
    end if;
    if not found or current_head.revision_id <> new.previous_revision_id then
        raise exception 'workload identity policy predecessor is not the exact current head'
            using errcode = '23514';
    end if;
    select *
      into previous_revision
      from workload_identity_policy_revisions revision
     where revision.organization_id = new.organization_id
       and revision.policy_id = new.policy_id
       and revision.id = new.previous_revision_id
       for key share of revision;
    if not found
       or new.revision_number <> previous_revision.revision_number + 1
       or new.installation_id <> previous_revision.installation_id
       or new.project_id <> previous_revision.project_id
       or new.environment_id <> previous_revision.environment_id
       or new.workload_id <> previous_revision.workload_id
       or new.accepted_at < previous_revision.accepted_at then
        raise exception 'workload identity policy is not the exact stable-owner successor'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger workload_identity_policy_revisions_transition_guard
before insert or update or delete on workload_identity_policy_revisions
for each row execute function validate_workload_identity_policy_revision_insert();

create function validate_workload_identity_policy_head_transition()
returns trigger
language plpgsql
as $$
declare
    accepted workload_identity_policy_revisions%rowtype;
begin
    if tg_op = 'DELETE' then
        raise exception 'workload identity policy heads are not deletable'
            using errcode = '23514';
    end if;
    perform 1
      from cloud_installations installation
     where installation.id = new.installation_id
       and installation.singleton_key
       for update of installation;
    if not found then
        raise exception 'workload identity policy head has no canonical Installation'
            using errcode = '23503';
    end if;
    select *
      into accepted
      from workload_identity_policy_revisions revision
     where revision.organization_id = new.organization_id
       and revision.policy_id = new.policy_id
       and revision.id = new.revision_id
       and revision.revision_number = new.revision_number
       for key share of revision;
    if not found then
        raise exception 'workload identity policy head has no exact accepted revision'
            using errcode = '23503';
    end if;
    if tg_op = 'INSERT' then
        if new.revision_number <> 1 or accepted.previous_revision_id is not null then
            raise exception 'initial workload identity policy head must select revision one'
                using errcode = '23514';
        end if;
        return new;
    end if;
    if new.installation_id is distinct from old.installation_id
       or new.organization_id is distinct from old.organization_id
       or new.project_id is distinct from old.project_id
       or new.environment_id is distinct from old.environment_id
       or new.policy_id is distinct from old.policy_id
       or new.workload_id is distinct from old.workload_id
       or new.revision_number <> old.revision_number + 1
       or accepted.previous_revision_id is distinct from old.revision_id
       or new.updated_at < old.updated_at then
        raise exception 'workload identity policy head must advance to its exact successor'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger workload_identity_policy_heads_transition_guard
before insert or update or delete on workload_identity_policy_heads
for each row execute function validate_workload_identity_policy_head_transition();

create index trust_domain_revisions_history_idx
    on trust_domain_revisions (
        installation_id,
        trust_domain_id,
        revision_number desc,
        id desc
    );

create index workload_identity_policy_revisions_history_idx
    on workload_identity_policy_revisions (
        organization_id,
        policy_id,
        revision_number desc,
        id desc
    );

comment on table trust_domain_revisions is
    'Identity-owned immutable canonical ACL history for installation-scoped workload trust domains';
comment on table trust_domain_heads is
    'One strongly consistent current revision per workload trust domain; names are stable and unique per Installation';
comment on table workload_identity_policy_revisions is
    'Identity-owned immutable tenant workload policy history bound to one exact trust-domain revision and owner lineage';
comment on table workload_identity_policy_heads is
    'One strongly consistent current workload identity policy per policy aggregate and logical Workload';
