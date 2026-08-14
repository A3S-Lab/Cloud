create table notifications (
    organization_id uuid not null references organizations(id),
    id uuid not null,
    recipient_principal_id uuid not null references identity_principals(id),
    source_event_id uuid not null references outbox_events(event_id),
    source_event_key text not null check (
        char_length(source_event_key) between 1 and 255
        and source_event_key ~ '^[a-z]+([a-z-]*[a-z])?(\.[a-z]+([a-z-]*[a-z])?){2,}$'
    ),
    source_schema_version integer not null check (source_schema_version > 0),
    source_aggregate_id uuid not null,
    source_aggregate_version bigint not null check (source_aggregate_version > 0),
    correlation_id uuid not null,
    severity text not null check (severity in ('information', 'warning', 'critical')),
    title text not null check (
        char_length(title) between 1 and 160
        and title = btrim(title)
        and title !~ '[[:cntrl:]]'
    ),
    body text not null check (
        char_length(body) between 1 and 2000
        and body = btrim(body)
        and body !~ '[[:cntrl:]]'
    ),
    scope_kind text not null check (
        scope_kind in ('organization', 'project', 'environment', 'node')
    ),
    project_id uuid,
    environment_id uuid,
    node_id uuid,
    occurred_at timestamptz not null,
    delivered_at timestamptz not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    read_at timestamptz,
    primary key (organization_id, id),
    unique (source_event_id, recipient_principal_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (delivered_at >= occurred_at),
    check (
        (scope_kind = 'organization'
            and project_id is null and environment_id is null and node_id is null)
        or (scope_kind = 'project'
            and project_id is not null and environment_id is null and node_id is null)
        or (scope_kind = 'environment'
            and project_id is not null and environment_id is not null and node_id is null)
        or (scope_kind = 'node'
            and project_id is null and environment_id is null and node_id is not null)
    ),
    check (
        (aggregate_version = 1 and read_at is null)
        or (aggregate_version = 2 and read_at is not null and read_at >= delivered_at)
    )
);

create index notifications_recipient_feed_idx
    on notifications (
        organization_id,
        recipient_principal_id,
        occurred_at desc,
        id desc
    );

create index notifications_recipient_unread_idx
    on notifications (
        organization_id,
        recipient_principal_id,
        occurred_at desc,
        id desc
    )
    where read_at is null;

create function validate_notification_source_event()
returns trigger
language plpgsql
as $$
declare
    source outbox_events%rowtype;
begin
    select * into source
      from outbox_events
     where event_id = new.source_event_id;

    if not found
       or source.organization_id <> new.organization_id
       or source.event_key <> new.source_event_key
       or source.schema_version <> new.source_schema_version
       or source.aggregate_id <> new.source_aggregate_id
       or source.aggregate_version <> new.source_aggregate_version
       or source.correlation_id <> new.correlation_id
       or source.occurred_at <> new.occurred_at then
        raise exception 'Notification source event identity is inconsistent';
    end if;
    return new;
end
$$;

create trigger notifications_validate_source_event
before insert on notifications
for each row execute function validate_notification_source_event();

create function enforce_notification_read_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Notifications cannot be deleted';
    end if;
    if new.organization_id is distinct from old.organization_id
       or new.id is distinct from old.id
       or new.recipient_principal_id is distinct from old.recipient_principal_id
       or new.source_event_id is distinct from old.source_event_id
       or new.source_event_key is distinct from old.source_event_key
       or new.source_schema_version is distinct from old.source_schema_version
       or new.source_aggregate_id is distinct from old.source_aggregate_id
       or new.source_aggregate_version is distinct from old.source_aggregate_version
       or new.correlation_id is distinct from old.correlation_id
       or new.severity is distinct from old.severity
       or new.title is distinct from old.title
       or new.body is distinct from old.body
       or new.scope_kind is distinct from old.scope_kind
       or new.project_id is distinct from old.project_id
       or new.environment_id is distinct from old.environment_id
       or new.node_id is distinct from old.node_id
       or new.occurred_at is distinct from old.occurred_at
       or new.delivered_at is distinct from old.delivered_at
       or old.aggregate_version <> 1
       or old.read_at is not null
       or new.aggregate_version <> 2
       or new.read_at is null
       or new.read_at < old.delivered_at then
        raise exception 'Notification mutation is not an unread-to-read transition';
    end if;
    return new;
end
$$;

create trigger notifications_read_transition_only
before update or delete on notifications
for each row execute function enforce_notification_read_transition();

comment on table notifications is
    'Deduplicated per-Principal in-app projections of committed transactional outbox facts; not a business-operation authority or provider queue';
