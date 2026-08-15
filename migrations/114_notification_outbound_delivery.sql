create table notification_outbound_subscriptions (
    organization_id uuid not null references organizations(id),
    id uuid not null,
    recipient_principal_id uuid not null references identity_principals(id),
    channel text not null
        check (channel in ('signed_webhook', 'slack_compatible')),
    minimum_severity text not null
        check (minimum_severity in ('information', 'warning', 'critical')),
    connector_project_id uuid not null,
    connector_environment_id uuid not null,
    connector_profile_id uuid not null,
    connector_revision_id uuid not null,
    definition_schema text not null
        check (definition_schema = 'cloud.notification.outbound-subscription.v1'),
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
    foreign key (
        organization_id,
        connector_project_id,
        connector_environment_id,
        connector_profile_id,
        connector_revision_id
    ) references connector_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        id
    ),
    check (created_by = recipient_principal_id),
    check (
        aggregate_version = 1 and revoked_at is null
        or aggregate_version = 2 and revoked_at >= created_at
    )
);

create unique index notification_outbound_subscriptions_active_target_idx
    on notification_outbound_subscriptions (
        organization_id,
        recipient_principal_id,
        channel,
        connector_project_id,
        connector_environment_id,
        connector_profile_id,
        connector_revision_id
    )
    where revoked_at is null;

create index notification_outbound_subscriptions_recipient_idx
    on notification_outbound_subscriptions (
        organization_id,
        recipient_principal_id,
        created_at desc,
        id desc
    );

create table notification_outbound_deliveries (
    organization_id uuid not null,
    id uuid not null,
    notification_id uuid not null,
    recipient_principal_id uuid not null,
    subscription_id uuid not null,
    requested_event_id uuid not null references outbox_events(event_id),
    payload_digest text not null
        check (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    channel text not null
        check (channel in ('signed_webhook', 'slack_compatible')),
    connector_project_id uuid not null,
    connector_environment_id uuid not null,
    connector_profile_id uuid not null,
    connector_revision_id uuid not null,
    occurred_at timestamptz not null,
    terminal_outcome text
        check (terminal_outcome in ('delivered', 'rejected', 'indeterminate')),
    terminal_generation bigint
        check (terminal_generation between 1 and 1000),
    terminal_attempt_id uuid,
    terminal_at timestamptz,
    primary key (organization_id, id),
    unique (requested_event_id),
    foreign key (organization_id, notification_id)
        references notifications (organization_id, id),
    foreign key (organization_id, recipient_principal_id, subscription_id)
        references notification_outbound_subscriptions (
            organization_id,
            recipient_principal_id,
            id
        ),
    foreign key (
        organization_id,
        connector_project_id,
        connector_environment_id,
        connector_profile_id,
        connector_revision_id
    ) references connector_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        id
    ),
    foreign key (
        organization_id,
        connector_project_id,
        connector_environment_id,
        connector_profile_id,
        connector_revision_id,
        terminal_attempt_id
    ) references connector_execution_attempts (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id,
        attempt_id
    ),
    check (
        terminal_outcome is null
        and terminal_generation is null
        and terminal_attempt_id is null
        and terminal_at is null
        or terminal_outcome is not null
        and terminal_generation is not null
        and terminal_attempt_id is not null
        and terminal_at >= occurred_at
    )
);

create index notification_outbound_deliveries_notification_idx
    on notification_outbound_deliveries (
        organization_id,
        notification_id,
        id
    );

create function enforce_notification_outbound_subscription_transition()
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
       or new.canonical_acl is distinct from old.canonical_acl
       or new.definition_digest is distinct from old.definition_digest
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'Outbound notification subscription mutation is not an active-to-revoked transition';
    end if;
    return new;
end
$$;

create trigger notification_outbound_subscription_revoke_only
before update or delete on notification_outbound_subscriptions
for each row execute function enforce_notification_outbound_subscription_transition();

create function validate_notification_outbound_delivery_fact()
returns trigger
language plpgsql
as $$
declare
    inbox notifications%rowtype;
    subscription notification_outbound_subscriptions%rowtype;
    requested outbox_events%rowtype;
    inbox_severity_rank integer;
    minimum_severity_rank integer;
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
       or inbox_severity_rank < minimum_severity_rank
       or requested.event_id is null
       or requested.event_key <> 'notification.delivery.requested'
       or requested.schema_version <> 1
       or requested.organization_id <> new.organization_id
       or requested.aggregate_id <> new.id
       or requested.aggregate_version <> 1
       or requested.occurred_at <> new.occurred_at
       or requested.correlation_id <> inbox.correlation_id
       or requested.causation_id is distinct from inbox.source_event_id then
        raise exception 'Outbound notification delivery fact is not authorized by its exact inbox projection and subscription';
    end if;
    return new;
end
$$;

create trigger notification_outbound_delivery_validate_fact
before insert on notification_outbound_deliveries
for each row execute function validate_notification_outbound_delivery_fact();

create function enforce_notification_outbound_delivery_transition()
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

create trigger notification_outbound_delivery_terminal_only
before update or delete on notification_outbound_deliveries
for each row execute function enforce_notification_outbound_delivery_transition();

create function validate_notification_outbound_terminal_receipt()
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

    if new.terminal_outcome = 'delivered'
       and not (
           attempt_state = 'terminal'
           and evidence_outcome = 'accepted'
           and new.terminal_at = evidence_completed_at
       )
       or new.terminal_outcome = 'rejected'
       and not (
           attempt_state = 'terminal'
           and evidence_outcome = 'rejected'
           and new.terminal_at = evidence_completed_at
       )
       or new.terminal_outcome = 'indeterminate'
       and not (
           attempt_state = 'dispatching'
           and evidence_outcome is null
           and attempt_deadline = new.terminal_at
       ) then
        raise exception 'Outbound notification terminal receipt does not match its exact C6 attempt';
    end if;
    return null;
end
$$;

create constraint trigger notification_outbound_delivery_terminal_receipt_exact
after update on notification_outbound_deliveries
deferrable initially deferred
for each row execute function validate_notification_outbound_terminal_receipt();

comment on table notification_outbound_subscriptions is
    'Notification-owned immutable personal ACL subscriptions with revoke-only lifecycle and exact Connector references; not another Connector, Secret, recipient directory, template, or provider authority';

comment on table notification_outbound_deliveries is
    'Notification-owned delivery authorization facts and monotonic logical terminal receipts; not a queue, retry schedule, retry counter, provider response store, or acknowledgement transport';

comment on column notification_outbound_deliveries.payload_digest is
    'SHA-256 of the exact bounded notification delivery payload; bodies, endpoints, credentials, signing inputs, and provider responses are never stored here';
