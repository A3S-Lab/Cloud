alter table notification_outbound_subscriptions
    drop constraint notification_outbound_subscriptions_definition_schema_check;

alter table notification_outbound_subscriptions
    add column maximum_provider_attempts bigint not null default 8,
    add constraint notification_outbound_subscriptions_definition_budget_check
        check (
            maximum_provider_attempts between 1 and 8
            and (
                definition_schema = 'cloud.notification.outbound-subscription.v1'
                and maximum_provider_attempts = 8
                or definition_schema = 'cloud.notification.outbound-subscription.v2'
            )
        );

alter table notification_outbound_subscriptions
    alter column maximum_provider_attempts drop default;

alter table notification_outbound_deliveries
    add column maximum_provider_attempts bigint not null default 8,
    add constraint notification_outbound_deliveries_definition_budget_check
        check (maximum_provider_attempts between 1 and 8),
    add constraint notification_outbound_deliveries_terminal_budget_check
        check (
            terminal_generation is null
            or terminal_generation <= maximum_provider_attempts
        );

alter table notification_outbound_deliveries
    alter column maximum_provider_attempts drop default;

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
        else 0
    end;
    expected_payload_schema := case subscription.definition_schema
        when 'cloud.notification.outbound-subscription.v1' then 'a3s.cloud.notification-delivery.v1'
        when 'cloud.notification.outbound-subscription.v2' then 'a3s.cloud.notification-delivery.v2'
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
        raise exception 'Outbound notification delivery fact is not authorized by its exact inbox projection and versioned subscription budget';
    end if;
    return new;
end
$$;

create or replace function enforce_notification_outbound_delivery_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Outbound notification deliveries cannot be deleted';
    end if;
    if old.terminal_outcome is not null
       or old.terminal_generation is not null
       or old.terminal_attempt_id is not null
       or old.terminal_at is not null
       or new.terminal_outcome is null
       or new.terminal_generation is null
       or new.terminal_attempt_id is null
       or new.terminal_at is null
       or new.organization_id is distinct from old.organization_id
       or new.id is distinct from old.id
       or new.notification_id is distinct from old.notification_id
       or new.recipient_principal_id is distinct from old.recipient_principal_id
       or new.subscription_id is distinct from old.subscription_id
       or new.requested_event_id is distinct from old.requested_event_id
       or new.payload_digest is distinct from old.payload_digest
       or new.maximum_provider_attempts is distinct from old.maximum_provider_attempts
       or new.channel is distinct from old.channel
       or new.connector_project_id is distinct from old.connector_project_id
       or new.connector_environment_id is distinct from old.connector_environment_id
       or new.connector_profile_id is distinct from old.connector_profile_id
       or new.connector_revision_id is distinct from old.connector_revision_id
       or new.occurred_at is distinct from old.occurred_at then
        raise exception 'Outbound notification delivery mutation is not a pending-to-terminal receipt transition';
    end if;
    return new;
end
$$;

create or replace function validate_notification_outbound_terminal_receipt()
returns trigger
language plpgsql
as $$
declare
    attempt_state text;
    attempt_deadline timestamptz;
    evidence_outcome text;
    evidence_completed_at timestamptz;
begin
    if new.terminal_outcome is null then
        return null;
    end if;
    select state, outcome_deadline_at
      into attempt_state, attempt_deadline
      from connector_execution_attempts
     where organization_id = new.organization_id
       and project_id = new.connector_project_id
       and environment_id = new.connector_environment_id
       and profile_id = new.connector_profile_id
       and revision_id = new.connector_revision_id
       and attempt_id = new.terminal_attempt_id;
    select outcome, completed_at
      into evidence_outcome, evidence_completed_at
      from connector_execution_evidence
     where organization_id = new.organization_id
       and project_id = new.connector_project_id
       and environment_id = new.connector_environment_id
       and profile_id = new.connector_profile_id
       and revision_id = new.connector_revision_id
       and attempt_id = new.terminal_attempt_id;

    if new.terminal_generation > new.maximum_provider_attempts
       or new.terminal_outcome = 'delivered'
       and (
           attempt_state is distinct from 'terminal'
           or evidence_outcome is distinct from 'accepted'
           or new.terminal_at is distinct from evidence_completed_at
       )
       or new.terminal_outcome = 'rejected'
       and (
           attempt_state is distinct from 'terminal'
           or evidence_outcome is distinct from 'rejected'
           or new.terminal_at is distinct from evidence_completed_at
       )
       or new.terminal_outcome = 'indeterminate'
       and (
           attempt_state is distinct from 'dispatching'
           or evidence_outcome is not null
           or attempt_deadline is distinct from new.terminal_at
       )
       or new.terminal_outcome = 'exhausted'
       and (
           attempt_state is distinct from 'terminal'
           or evidence_outcome is distinct from 'retryable'
           or new.terminal_at is distinct from evidence_completed_at
           or new.terminal_generation is distinct from new.maximum_provider_attempts
       ) then
        raise exception 'Outbound notification terminal receipt does not match its exact C6 attempt and pinned delivery budget';
    end if;
    return null;
end
$$;

comment on column notification_outbound_subscriptions.maximum_provider_attempts is
    'Immutable ACL-owned provider-attempt budget; v1 means exactly eight and v2 explicitly pins a value from one through eight';

comment on column notification_outbound_deliveries.maximum_provider_attempts is
    'Immutable copy of the authorizing subscription budget used by replay and terminal receipt validation without a mutable counter';

comment on column notification_outbound_deliveries.terminal_outcome is
    'Monotonic logical result whose generation cannot exceed the immutable delivery budget; Exhausted must equal that exact budget and matching retryable C6 evidence';
