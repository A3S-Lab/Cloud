create table notification_alert_policies (
    organization_id uuid not null references organizations(id),
    id uuid not null,
    recipient_principal_id uuid not null references identity_principals(id),
    source text not null
        check (source = 'edge.domain-claim-status.v1'),
    project_id uuid not null,
    environment_id uuid not null,
    notify_on_recovery boolean not null,
    definition_schema text not null
        check (definition_schema = 'cloud.notification.alert-policy.v1'),
    canonical_acl text not null
        check (octet_length(canonical_acl) between 1 and 16384),
    definition_digest text not null
        check (definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null
        check (aggregate_version in (1, 2)),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    revoked_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, recipient_principal_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (created_by = recipient_principal_id),
    check (
        aggregate_version = 1 and revoked_at is null
        or aggregate_version = 2 and revoked_at >= created_at
    )
);

create unique index notification_alert_policies_active_source_scope_idx
    on notification_alert_policies (
        organization_id,
        recipient_principal_id,
        source,
        project_id,
        environment_id
    )
    where revoked_at is null;

create index notification_alert_policies_recipient_idx
    on notification_alert_policies (
        organization_id,
        recipient_principal_id,
        created_at desc,
        id desc
    );

create index notification_alert_policies_source_scope_idx
    on notification_alert_policies (
        organization_id,
        source,
        project_id,
        environment_id,
        created_at,
        id
    )
    where revoked_at is null;

create function enforce_notification_alert_policy_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Notification alert policies cannot be deleted';
    end if;
    if old.aggregate_version <> 1
       or old.revoked_at is not null
       or new.aggregate_version <> 2
       or new.revoked_at < old.created_at
       or new.organization_id is distinct from old.organization_id
       or new.id is distinct from old.id
       or new.recipient_principal_id is distinct from old.recipient_principal_id
       or new.source is distinct from old.source
       or new.project_id is distinct from old.project_id
       or new.environment_id is distinct from old.environment_id
       or new.notify_on_recovery is distinct from old.notify_on_recovery
       or new.definition_schema is distinct from old.definition_schema
       or new.canonical_acl is distinct from old.canonical_acl
       or new.definition_digest is distinct from old.definition_digest
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'Notification alert policy mutation is not an active-to-revoked transition';
    end if;
    return new;
end
$$;

create trigger notification_alert_policy_revoke_only
before update or delete on notification_alert_policies
for each row execute function enforce_notification_alert_policy_transition();

comment on table notification_alert_policies is
    'Notification-owned immutable personal A3S ACL policies over a compile-time closed owner-event registry; not a metric store, incident authority, poller, timer, scheduler, queue, or second event rail';

comment on column notification_alert_policies.notify_on_recovery is
    'Allows an owner-emitted recovery fact only after the same recipient and aggregate has a policy-covered projected firing fact';
