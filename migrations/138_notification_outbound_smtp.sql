alter table notification_outbound_subscriptions
    alter column connector_project_id drop not null,
    alter column connector_environment_id drop not null,
    alter column connector_profile_id drop not null,
    alter column connector_revision_id drop not null,
    add column recipient_contact_id uuid,
    drop constraint notification_outbound_subscriptions_channel_check,
    drop constraint notification_outbound_subscriptions_definition_policy_check,
    add constraint notification_outbound_subscriptions_channel_check
        check (channel in ('signed_webhook', 'slack_compatible', 'smtp')),
    add constraint notification_outbound_subscriptions_target_authority_check
        check (
            channel in ('signed_webhook', 'slack_compatible')
            and connector_project_id is not null
            and connector_environment_id is not null
            and connector_profile_id is not null
            and connector_revision_id is not null
            and recipient_contact_id is null
            or channel = 'smtp'
            and connector_project_id is null
            and connector_environment_id is null
            and connector_profile_id is null
            and connector_revision_id is null
            and recipient_contact_id is not null
        ),
    add constraint notification_outbound_subscriptions_recipient_contact_fk
        foreign key (recipient_principal_id, recipient_contact_id)
        references recipient_contacts (principal_id, id),
    add constraint notification_outbound_subscriptions_definition_policy_check
        check (
            definition_schema = 'cloud.notification.outbound-subscription.v1'
            and channel in ('signed_webhook', 'slack_compatible')
            and maximum_provider_attempts = 8
            and suppress_before is null
            or definition_schema = 'cloud.notification.outbound-subscription.v2'
            and channel in ('signed_webhook', 'slack_compatible')
            and maximum_provider_attempts between 1 and 8
            and suppress_before is null
            or definition_schema = 'cloud.notification.outbound-subscription.v3'
            and channel in ('signed_webhook', 'slack_compatible')
            and maximum_provider_attempts between 1 and 8
            and suppress_before is not null
            and suppress_before > created_at
            and suppress_before <= created_at + interval '30 days'
            or definition_schema = 'cloud.notification.outbound-subscription.v4'
            and channel = 'smtp'
            and maximum_provider_attempts between 1 and 8
            and (
                suppress_before is null
                or suppress_before > created_at
                and suppress_before <= created_at + interval '30 days'
            )
        );

create unique index notification_outbound_subscriptions_active_contact_idx
    on notification_outbound_subscriptions (
        organization_id,
        recipient_principal_id,
        channel,
        recipient_contact_id
    )
    where revoked_at is null and channel = 'smtp';

alter table notification_outbound_deliveries
    alter column connector_project_id drop not null,
    alter column connector_environment_id drop not null,
    alter column connector_profile_id drop not null,
    alter column connector_revision_id drop not null,
    add column recipient_contact_id uuid,
    drop constraint notification_outbound_deliveries_channel_check,
    drop constraint notification_outbound_deliveries_terminal_outcome_check,
    add constraint notification_outbound_deliveries_channel_check
        check (channel in ('signed_webhook', 'slack_compatible', 'smtp')),
    add constraint notification_outbound_deliveries_target_authority_check
        check (
            channel in ('signed_webhook', 'slack_compatible')
            and connector_project_id is not null
            and connector_environment_id is not null
            and connector_profile_id is not null
            and connector_revision_id is not null
            and recipient_contact_id is null
            or channel = 'smtp'
            and connector_project_id is null
            and connector_environment_id is null
            and connector_profile_id is null
            and connector_revision_id is null
            and recipient_contact_id is not null
        ),
    add constraint notification_outbound_deliveries_recipient_contact_fk
        foreign key (recipient_principal_id, recipient_contact_id)
        references recipient_contacts (principal_id, id),
    add constraint notification_outbound_deliveries_terminal_outcome_check
        check (
            terminal_outcome in (
                'delivered',
                'rejected',
                'indeterminate',
                'exhausted',
                'obsolete'
            )
        );

create table notification_outbound_smtp_attempts (
    organization_id uuid not null,
    delivery_id uuid not null,
    recipient_contact_id uuid not null references recipient_contacts(id),
    generation bigint not null check (generation between 1 and 8),
    attempt_id uuid not null check (
        attempt_id <> '00000000-0000-0000-0000-000000000000'
    ),
    state text not null check (state in ('reserved', 'dispatching', 'terminal')),
    outcome text check (
        outcome in ('accepted', 'rejected', 'retryable', 'indeterminate', 'obsolete')
    ),
    fence_generation bigint not null check (fence_generation > 0),
    fence_token uuid not null check (
        fence_token <> '00000000-0000-0000-0000-000000000000'
    ),
    reserved_at timestamptz not null,
    lease_expires_at timestamptz not null,
    dispatch_started_at timestamptz,
    outcome_deadline_at timestamptz,
    completed_at timestamptz,
    primary key (organization_id, delivery_id, generation),
    unique (organization_id, delivery_id, recipient_contact_id, attempt_id),
    foreign key (organization_id, delivery_id)
        references notification_outbound_deliveries (organization_id, id),
    check (
        lease_expires_at > reserved_at
        and lease_expires_at <= reserved_at + interval '5 minutes'
    ),
    check (
        state = 'reserved'
        and outcome is null
        and dispatch_started_at is null
        and outcome_deadline_at is null
        and completed_at is null
        or state = 'dispatching'
        and outcome is null
        and dispatch_started_at >= reserved_at
        and dispatch_started_at < lease_expires_at
        and outcome_deadline_at > dispatch_started_at
        and outcome_deadline_at <= dispatch_started_at + interval '120 seconds'
        and completed_at is null
        or state = 'terminal'
        and outcome in ('accepted', 'rejected', 'retryable', 'indeterminate')
        and dispatch_started_at >= reserved_at
        and dispatch_started_at < lease_expires_at
        and outcome_deadline_at > dispatch_started_at
        and outcome_deadline_at <= dispatch_started_at + interval '120 seconds'
        and completed_at >= dispatch_started_at
        or state = 'terminal'
        and outcome = 'obsolete'
        and dispatch_started_at is null
        and outcome_deadline_at is null
        and completed_at between reserved_at and lease_expires_at
    )
);

alter table notification_outbound_deliveries
    add constraint notification_outbound_deliveries_smtp_attempt_fk
    foreign key (
        organization_id,
        id,
        recipient_contact_id,
        terminal_attempt_id
    ) references notification_outbound_smtp_attempts (
        organization_id,
        delivery_id,
        recipient_contact_id,
        attempt_id
    );

create index notification_outbound_smtp_attempts_recovery_idx
    on notification_outbound_smtp_attempts (
        outcome_deadline_at,
        organization_id,
        delivery_id,
        generation
    )
    where state = 'dispatching';

create function validate_notification_outbound_smtp_attempt_insert()
returns trigger
language plpgsql
as $$
declare
    delivery_channel text;
    delivery_contact_id uuid;
    delivery_budget bigint;
    delivery_terminal_outcome text;
    prior_state text;
    prior_outcome text;
begin
    select channel, recipient_contact_id, maximum_provider_attempts, terminal_outcome
      into delivery_channel, delivery_contact_id, delivery_budget, delivery_terminal_outcome
      from notification_outbound_deliveries
     where organization_id = new.organization_id
       and id = new.delivery_id
     for update;

    if delivery_channel is distinct from 'smtp'
       or delivery_contact_id is distinct from new.recipient_contact_id
       or delivery_terminal_outcome is not null
       or new.generation > delivery_budget
       or new.state not in ('reserved', 'terminal')
       or new.state = 'terminal' and new.outcome is distinct from 'obsolete' then
        raise exception 'SMTP notification attempt is not authorized by its exact pending delivery';
    end if;

    if new.generation = 1 then
        if exists (
            select 1
              from notification_outbound_smtp_attempts
             where organization_id = new.organization_id
               and delivery_id = new.delivery_id
        ) then
            raise exception 'SMTP notification first attempt is not the first generation';
        end if;
    else
        select state, outcome
          into prior_state, prior_outcome
          from notification_outbound_smtp_attempts
         where organization_id = new.organization_id
           and delivery_id = new.delivery_id
           and generation = new.generation - 1;
        if prior_state is distinct from 'terminal'
           or prior_outcome is distinct from 'retryable' then
            raise exception 'SMTP notification attempt requires exact prior retryable evidence';
        end if;
    end if;
    return new;
end
$$;

create trigger notification_outbound_smtp_attempt_validate_insert
before insert on notification_outbound_smtp_attempts
for each row execute function validate_notification_outbound_smtp_attempt_insert();

create function enforce_notification_outbound_smtp_attempt_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'SMTP notification attempts cannot be deleted';
    end if;
    if old.organization_id <> new.organization_id
       or old.delivery_id <> new.delivery_id
       or old.recipient_contact_id <> new.recipient_contact_id
       or old.generation <> new.generation
       or old.attempt_id <> new.attempt_id then
        raise exception 'SMTP notification attempt identity is immutable';
    end if;
    if old.state = 'terminal' then
        raise exception 'Terminal SMTP notification attempts are immutable';
    end if;
    if old.state = 'reserved' and new.state = 'reserved' then
        if new.fence_generation <> old.fence_generation + 1
           or new.fence_token = old.fence_token
           or new.reserved_at < old.lease_expires_at
           or new.dispatch_started_at is not null
           or new.outcome_deadline_at is not null
           or new.completed_at is not null
           or new.outcome is not null then
            raise exception 'SMTP notification reservation takeover is not fenced';
        end if;
    elsif old.state = 'reserved' and new.state in ('dispatching', 'terminal') then
        if new.fence_generation <> old.fence_generation
           or new.fence_token <> old.fence_token
           or new.reserved_at <> old.reserved_at
           or new.lease_expires_at <> old.lease_expires_at
           or new.state = 'terminal' and new.outcome is distinct from 'obsolete' then
            raise exception 'SMTP notification dispatch transition uses a stale fence';
        end if;
    elsif old.state = 'dispatching' and new.state = 'terminal' then
        if new.fence_generation <> old.fence_generation
           or new.fence_token <> old.fence_token
           or new.reserved_at <> old.reserved_at
           or new.lease_expires_at <> old.lease_expires_at
           or new.dispatch_started_at <> old.dispatch_started_at
           or new.outcome_deadline_at <> old.outcome_deadline_at
           or new.outcome not in (
               'accepted', 'rejected', 'retryable', 'indeterminate'
           ) then
            raise exception 'SMTP notification settlement uses a stale fence';
        end if;
    else
        raise exception 'SMTP notification attempt transition is invalid';
    end if;
    return new;
end
$$;

create trigger notification_outbound_smtp_attempt_transition
before update or delete on notification_outbound_smtp_attempts
for each row execute function enforce_notification_outbound_smtp_attempt_transition();

comment on table notification_outbound_smtp_attempts is
    'Notifications-owned SMTP provider-attempt leases, dispatch fences, and bounded outcome evidence; not a queue, scheduler, Connector attempt, mailbox store, credential store, or provider-response store';

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
       or new.recipient_contact_id is distinct from old.recipient_contact_id
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
        when 'cloud.notification.outbound-subscription.v4' then 3
        else 0
    end;
    expected_payload_schema := case subscription.definition_schema
        when 'cloud.notification.outbound-subscription.v1' then 'a3s.cloud.notification-delivery.v1'
        when 'cloud.notification.outbound-subscription.v2' then 'a3s.cloud.notification-delivery.v2'
        when 'cloud.notification.outbound-subscription.v3' then 'a3s.cloud.notification-delivery.v2'
        when 'cloud.notification.outbound-subscription.v4' then 'a3s.cloud.notification-delivery.v3'
        else ''
    end;

    if inbox.id is null
       or inbox.recipient_principal_id <> new.recipient_principal_id
       or inbox.occurred_at <> new.occurred_at
       or subscription.id is null
       or subscription.revoked_at is not null
       or subscription.channel <> new.channel
       or subscription.connector_project_id is distinct from new.connector_project_id
       or subscription.connector_environment_id is distinct from new.connector_environment_id
       or subscription.connector_profile_id is distinct from new.connector_profile_id
       or subscription.connector_revision_id is distinct from new.connector_revision_id
       or subscription.recipient_contact_id is distinct from new.recipient_contact_id
       or subscription.maximum_provider_attempts <> new.maximum_provider_attempts
       or inbox_severity_rank < minimum_severity_rank
       or subscription.suppress_before is not null
          and inbox.occurred_at < subscription.suppress_before
       or requested.event_id is null
       or requested.event_key <> 'notification.delivery.requested'
       or requested.schema_version <> expected_schema_version
       or requested.payload ->> 'schema' is distinct from expected_payload_schema
       or expected_schema_version = 1
          and requested.payload ? 'maximumProviderAttempts'
       or expected_schema_version > 1
          and (requested.payload ->> 'maximumProviderAttempts')::bigint
              is distinct from new.maximum_provider_attempts
       or expected_schema_version <= 2
          and (
              (requested.payload ->> 'projectId')::uuid
                  is distinct from new.connector_project_id
              or (requested.payload ->> 'environmentId')::uuid
                  is distinct from new.connector_environment_id
              or (requested.payload ->> 'targetProfileId')::uuid
                  is distinct from new.connector_profile_id
              or (requested.payload ->> 'targetRevisionId')::uuid
                  is distinct from new.connector_revision_id
              or requested.payload ? 'recipientContactId'
          )
       or expected_schema_version = 3
          and (
              (requested.payload ->> 'recipientContactId')::uuid
                  is distinct from new.recipient_contact_id
              or requested.payload ?| array[
                  'projectId',
                  'environmentId',
                  'targetProfileId',
                  'targetRevisionId',
                  'address',
                  'mailbox',
                  'addressDigest',
                  'contactHint',
                  'credentials',
                  'providerResponse',
                  'providerResponseText'
              ]
          )
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
       or new.recipient_contact_id is distinct from old.recipient_contact_id
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
    smtp_generation bigint;
begin
    if new.terminal_outcome is null then
        return null;
    end if;
    if new.terminal_generation > new.maximum_provider_attempts then
        raise exception 'Outbound notification terminal receipt exceeds its pinned delivery budget';
    end if;

    if new.channel = 'smtp' then
        select state, outcome_deadline_at, outcome, completed_at, generation
          into attempt_state, attempt_deadline, evidence_outcome,
               evidence_completed_at, smtp_generation
          from notification_outbound_smtp_attempts
         where organization_id = new.organization_id
           and delivery_id = new.id
           and recipient_contact_id = new.recipient_contact_id
           and attempt_id = new.terminal_attempt_id;

        if smtp_generation is distinct from new.terminal_generation
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
           or new.terminal_outcome = 'exhausted'
              and (
                  attempt_state is distinct from 'terminal'
                  or evidence_outcome is distinct from 'retryable'
                  or new.terminal_at is distinct from evidence_completed_at
                  or new.terminal_generation is distinct from new.maximum_provider_attempts
              )
           or new.terminal_outcome = 'obsolete'
              and (
                  attempt_state is distinct from 'terminal'
                  or evidence_outcome is distinct from 'obsolete'
                  or new.terminal_at is distinct from evidence_completed_at
              )
           or new.terminal_outcome = 'indeterminate'
              and (
                  attempt_state is distinct from 'terminal'
                  or evidence_outcome is distinct from 'indeterminate'
                  or new.terminal_at is distinct from evidence_completed_at
              )
           or new.terminal_outcome not in (
               'delivered', 'rejected', 'exhausted', 'obsolete', 'indeterminate'
           ) then
            raise exception 'Outbound SMTP terminal receipt does not match its exact Notifications attempt';
        end if;
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

    if new.terminal_outcome = 'delivered'
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
       )
       or new.terminal_outcome not in (
           'delivered', 'rejected', 'indeterminate', 'exhausted'
       ) then
        raise exception 'Outbound notification terminal receipt does not match its exact C6 attempt and pinned delivery budget';
    end if;
    return null;
end
$$;

comment on column notification_outbound_subscriptions.recipient_contact_id is
    'Opaque Identity-owned verified-contact reference for SMTP subscriptions; mailbox and verification material are forbidden';

comment on column notification_outbound_deliveries.recipient_contact_id is
    'Immutable opaque contact target copied from the authorizing SMTP subscription; no mailbox, digest, hint, or credential is stored';
