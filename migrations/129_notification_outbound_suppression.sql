alter table notification_outbound_subscriptions
    add column suppress_before timestamptz;

alter table notification_outbound_subscriptions
    drop constraint notification_outbound_subscriptions_definition_budget_check;

alter table notification_outbound_subscriptions
    add constraint notification_outbound_subscriptions_definition_policy_check
        check (
            (
                definition_schema = 'cloud.notification.outbound-subscription.v1'
                and maximum_provider_attempts = 8
                and suppress_before is null
            )
            or (
                definition_schema = 'cloud.notification.outbound-subscription.v2'
                and maximum_provider_attempts between 1 and 8
                and suppress_before is null
            )
            or (
                definition_schema = 'cloud.notification.outbound-subscription.v3'
                and maximum_provider_attempts between 1 and 8
                and suppress_before is not null
                and suppress_before > created_at
                and suppress_before <= created_at + interval '30 days'
            )
        );

create or replace function enforce_notification_outbound_subscription_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Outbound notification subscriptions cannot be deleted';
    end if;
    if old.aggregate_version <> 1
       or old.revoked_at is not null
       or new.aggregate_version <> 2
       or new.revoked_at < old.created_at
       or new.organization_id is distinct from old.organization_id
       or new.id is distinct from old.id
       or new.recipient_principal_id is distinct from old.recipient_principal_id
       or new.channel is distinct from old.channel
       or new.minimum_severity is distinct from old.minimum_severity
       or new.connector_project_id is distinct from old.connector_project_id
       or new.connector_environment_id is distinct from old.connector_environment_id
       or new.connector_profile_id is distinct from old.connector_profile_id
       or new.connector_revision_id is distinct from old.connector_revision_id
       or new.definition_schema is distinct from old.definition_schema
       or new.maximum_provider_attempts is distinct from old.maximum_provider_attempts
       or new.suppress_before is distinct from old.suppress_before
       or new.canonical_acl is distinct from old.canonical_acl
       or new.definition_digest is distinct from old.definition_digest
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'Outbound notification subscription mutation is not an active-to-revoked transition';
    end if;
    return new;
end
$$;

create or replace function validate_notification_outbound_delivery_fact()
returns trigger
language plpgsql
as $$
declare
    inbox notifications%rowtype;
    subscription notification_outbound_subscriptions%rowtype;
    requested outbox_events%rowtype;
    inbox_severity_rank integer;
    minimum_severity_rank integer;
    expected_schema_version integer;
    expected_payload_schema text;
begin
    select * into inbox
      from notifications
     where organization_id = new.organization_id
       and id = new.notification_id;
    select * into subscription
      from notification_outbound_subscriptions
     where organization_id = new.organization_id
       and recipient_principal_id = new.recipient_principal_id
       and id = new.subscription_id;
    select * into requested
      from outbox_events
     where event_id = new.requested_event_id;

    inbox_severity_rank := case inbox.severity
        when 'information' then 1
        when 'warning' then 2
        when 'critical' then 3
        else 0
    end;
    minimum_severity_rank := case subscription.minimum_severity
        when 'information' then 1
        when 'warning' then 2
        when 'critical' then 3
        else 4
    end;
    expected_schema_version := case subscription.definition_schema
        when 'cloud.notification.outbound-subscription.v1' then 1
        when 'cloud.notification.outbound-subscription.v2' then 2
        when 'cloud.notification.outbound-subscription.v3' then 2
        else 0
    end;
    expected_payload_schema := case subscription.definition_schema
        when 'cloud.notification.outbound-subscription.v1' then 'a3s.cloud.notification-delivery.v1'
        when 'cloud.notification.outbound-subscription.v2' then 'a3s.cloud.notification-delivery.v2'
        when 'cloud.notification.outbound-subscription.v3' then 'a3s.cloud.notification-delivery.v2'
        else ''
    end;

    if inbox.id is null
       or inbox.recipient_principal_id <> new.recipient_principal_id
       or inbox.occurred_at <> new.occurred_at
       or subscription.id is null
       or subscription.revoked_at is not null
       or subscription.channel <> new.channel
       or subscription.connector_project_id <> new.connector_project_id
       or subscription.connector_environment_id <> new.connector_environment_id
       or subscription.connector_profile_id <> new.connector_profile_id
       or subscription.connector_revision_id <> new.connector_revision_id
       or subscription.maximum_provider_attempts <> new.maximum_provider_attempts
       or inbox_severity_rank < minimum_severity_rank
       or (
           subscription.definition_schema = 'cloud.notification.outbound-subscription.v3'
           and inbox.occurred_at < subscription.suppress_before
       )
       or requested.event_id is null
       or requested.event_key <> 'notification.delivery.requested'
       or requested.schema_version <> expected_schema_version
       or requested.payload ->> 'schema' is distinct from expected_payload_schema
       or expected_schema_version = 1
          and requested.payload ? 'maximumProviderAttempts'
       or expected_schema_version = 2
          and (requested.payload ->> 'maximumProviderAttempts')::bigint
              is distinct from new.maximum_provider_attempts
       or requested.organization_id <> new.organization_id
       or requested.aggregate_id <> new.id
       or requested.aggregate_version <> 1
       or requested.occurred_at <> new.occurred_at
       or requested.correlation_id <> inbox.correlation_id
       or requested.causation_id is distinct from inbox.source_event_id then
        raise exception 'Outbound notification delivery fact is not authorized by its exact inbox projection and versioned subscription policy';
    end if;
    return new;
end
$$;

comment on column notification_outbound_subscriptions.suppress_before is
    'Immutable ACL-owned event-time cutoff; v3 suppresses outbound authorization strictly before this timestamp while retaining the personal inbox projection';
