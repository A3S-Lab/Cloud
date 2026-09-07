-- AUT0.1/AUT0.3: durable immutable Automation definition and revision owner.
-- The revision ACL is the only persisted semantic source. This catalog does
-- not own timers, queues, target execution, Secrets, or provider connections.

create table automation_definitions (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    automation_id uuid not null,
    current_revision_id uuid not null,
    current_revision_number bigint not null check (current_revision_number > 0),
    current_revision_digest text not null
        check (current_revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, automation_id),
    unique (automation_id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (created_at <= updated_at)
);

create table automation_revisions (
    organization_id uuid not null,
    automation_id uuid not null,
    revision_id uuid not null,
    revision_number bigint not null check (revision_number > 0),
    parent_revision_id uuid,
    parent_digest text,
    revision_digest text not null
        check (revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    revision_acl text not null
        check (octet_length(revision_acl) between 1 and 131072),
    created_at timestamptz not null,
    primary key (organization_id, revision_id),
    unique (revision_id),
    unique (organization_id, automation_id, revision_number),
    foreign key (organization_id, automation_id)
        references automation_definitions (organization_id, automation_id),
    foreign key (organization_id, parent_revision_id)
        references automation_revisions (organization_id, revision_id),
    check (
        (revision_number = 1 and parent_revision_id is null and parent_digest is null)
        or (revision_number > 1 and parent_revision_id is not null and parent_digest is not null)
    ),
    check (parent_digest is null or parent_digest ~ '^sha256:[0-9a-f]{64}$')
);

alter table automation_definitions
    add constraint automation_definitions_current_revision_fk
    foreign key (organization_id, current_revision_id)
    references automation_revisions (organization_id, revision_id)
    deferrable initially deferred;

create index automation_definitions_scope_idx
    on automation_definitions (
        organization_id,
        project_id,
        environment_id,
        automation_id
    );

create index automation_revisions_history_idx
    on automation_revisions (
        organization_id,
        automation_id,
        revision_number,
        revision_id
    );

create function enforce_automation_definition_head()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Automation definitions cannot be deleted';
    end if;
    if new.organization_id <> old.organization_id
       or new.project_id <> old.project_id
       or new.environment_id <> old.environment_id
       or new.automation_id <> old.automation_id
       or new.created_at <> old.created_at then
        raise exception 'Automation definition identity and creation time are immutable';
    end if;
    if new.current_revision_number <> old.current_revision_number + 1
       or new.current_revision_id = old.current_revision_id
       or new.current_revision_digest = old.current_revision_digest
       or new.updated_at < old.updated_at then
        raise exception 'Automation definition head must advance exactly one immutable revision';
    end if;
    if not exists (
        select 1
        from automation_revisions revision
        where revision.organization_id = new.organization_id
          and revision.automation_id = new.automation_id
          and revision.revision_id = new.current_revision_id
          and revision.revision_number = new.current_revision_number
          and revision.revision_digest = new.current_revision_digest
    ) then
        raise exception 'Automation definition head must reference its exact revision';
    end if;
    return new;
end
$$;

create trigger automation_definitions_validate_head
before update or delete on automation_definitions
for each row execute function enforce_automation_definition_head();

create function enforce_automation_revision_lineage()
returns trigger
language plpgsql
as $$
begin
    if new.revision_number > 1 and not exists (
        select 1
        from automation_revisions parent
        where parent.organization_id = new.organization_id
          and parent.automation_id = new.automation_id
          and parent.revision_id = new.parent_revision_id
          and parent.revision_number = new.revision_number - 1
          and parent.revision_digest = new.parent_digest
    ) then
        raise exception 'Automation revision parent must be the exact preceding digest';
    end if;
    return new;
end
$$;

create trigger automation_revisions_validate_lineage
before insert on automation_revisions
for each row execute function enforce_automation_revision_lineage();

create function reject_automation_revision_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Automation revisions are immutable';
end
$$;

create trigger automation_revisions_immutable
before update or delete on automation_revisions
for each row execute function reject_automation_revision_mutation();

comment on table automation_definitions is
    'Automations-owned immutable definition identity and current revision head; no timer or target state';

comment on table automation_revisions is
    'Automations-owned canonical revision ACL lineage; revisions are append-only and digest-linked';

comment on column automation_revisions.revision_acl is
    'Canonical cloud.automation.revision.v1 ACL restored and validated before use';
