alter table notification_alert_policies
    alter column project_id drop not null,
    alter column environment_id drop not null,
    add column node_id uuid,
    add constraint notification_alert_policies_node_fk
        foreign key (organization_id, node_id)
        references nodes (organization_id, id) not valid,
    drop constraint notification_alert_policies_source_check,
    drop constraint notification_alert_policies_definition_schema_check,
    add constraint notification_alert_policies_schema_source_target_check
    check (
        (
            definition_schema = 'cloud.notification.alert-policy.v1'
            and source in (
                'edge.domain-claim-status.v1',
                'edge.gateway-certificate-renewal-status.v1',
                'workload.deployment-health.v1',
                'edge.gateway-certificate-expiry-status.v1'
            )
            and project_id is not null
            and environment_id is not null
            and node_id is null
        )
        or
        (
            definition_schema = 'cloud.notification.alert-policy.v2'
            and source = 'fleet.node-availability-status.v1'
            and project_id is null
            and environment_id is null
            and node_id is not null
        )
    ) not valid;

alter table notification_alert_policies
    validate constraint notification_alert_policies_node_fk;

alter table notification_alert_policies
    validate constraint notification_alert_policies_schema_source_target_check;

drop index notification_alert_policies_active_source_scope_idx;
drop index notification_alert_policies_source_scope_idx;

create unique index notification_alert_policies_active_environment_source_scope_idx
    on notification_alert_policies (
        organization_id,
        recipient_principal_id,
        source,
        project_id,
        environment_id
    )
    where revoked_at is null
      and definition_schema = 'cloud.notification.alert-policy.v1';

create unique index notification_alert_policies_active_node_source_scope_idx
    on notification_alert_policies (
        organization_id,
        recipient_principal_id,
        source,
        node_id
    )
    where revoked_at is null
      and definition_schema = 'cloud.notification.alert-policy.v2';

create index notification_alert_policies_environment_source_scope_idx
    on notification_alert_policies (
        organization_id,
        source,
        project_id,
        environment_id,
        created_at,
        id
    )
    where revoked_at is null
      and definition_schema = 'cloud.notification.alert-policy.v1';

create index notification_alert_policies_node_source_scope_idx
    on notification_alert_policies (
        organization_id,
        source,
        node_id,
        created_at,
        id
    )
    where revoked_at is null
      and definition_schema = 'cloud.notification.alert-policy.v2';

create or replace function enforce_notification_alert_policy_transition()
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
       or new.node_id is distinct from old.node_id
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

comment on column notification_alert_policies.node_id is
    'Exact Fleet Node target for alert-policy v2; mutually exclusive with the legacy Environment target';

comment on column notification_alert_policies.source is
    'Compile-time closed typed owner-event source registry; schema and source determine an exact Environment-or-Node target';
