create table fleet_node_availability_fact_heads (
    organization_id uuid not null,
    node_id uuid not null,
    state text not null check (state in ('observed', 'unavailable', 'resolved')),
    node_aggregate_version bigint not null check (node_aggregate_version > 0),
    last_observed_at timestamptz not null,
    timeout_deadline_at timestamptz,
    latest_event_id uuid,
    latest_event_key text check (
        latest_event_key in (
            'fleet.node.unavailable',
            'fleet.node.availability-resolved'
        )
    ),
    latest_phase_version bigint check (latest_phase_version > 0),
    firing_event_id uuid,
    firing_phase_version bigint check (firing_phase_version > 0),
    firing_node_aggregate_version bigint check (firing_node_aggregate_version > 0),
    firing_last_observed_at timestamptz,
    firing_timeout_deadline_at timestamptz,
    detected_at timestamptz,
    resolved_at timestamptz,
    resolution_reason text check (
        resolution_reason in ('heartbeat_restored', 'node_revoked')
    ),
    updated_at timestamptz not null,
    primary key (organization_id, node_id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (
        timeout_deadline_at is null
        or timeout_deadline_at > last_observed_at
    ),
    check (
        (latest_event_id is null
            and latest_event_key is null
            and latest_phase_version is null)
        or
        (latest_event_id is not null
            and latest_event_key is not null
            and latest_phase_version is not null)
    ),
    check (
        (firing_event_id is null
            and firing_phase_version is null
            and firing_node_aggregate_version is null
            and firing_last_observed_at is null
            and firing_timeout_deadline_at is null
            and detected_at is null)
        or
        (firing_event_id is not null
            and firing_phase_version is not null
            and firing_node_aggregate_version is not null
            and firing_last_observed_at is not null
            and firing_timeout_deadline_at is not null
            and detected_at is not null
            and mod(firing_phase_version, 2) = 0
            and firing_node_aggregate_version = firing_phase_version / 2
            and firing_timeout_deadline_at > firing_last_observed_at
            and detected_at > firing_timeout_deadline_at)
    ),
    check (
        state = 'observed'
        and latest_event_id is null
        and firing_event_id is null
        and resolved_at is null
        and resolution_reason is null
        or
        state = 'unavailable'
        and latest_event_key = 'fleet.node.unavailable'
        and latest_event_id = firing_event_id
        and latest_phase_version = firing_phase_version
        and node_aggregate_version = firing_node_aggregate_version
        and last_observed_at = firing_last_observed_at
        and timeout_deadline_at = firing_timeout_deadline_at
        and resolved_at is null
        and resolution_reason is null
        or
        state = 'resolved'
        and latest_event_key = 'fleet.node.availability-resolved'
        and firing_event_id is not null
        and latest_event_id <> firing_event_id
        and latest_phase_version > firing_phase_version
        and mod(latest_phase_version, 2) = 1
        and resolved_at is not null
        and resolved_at >= detected_at
        and (
            resolution_reason = 'heartbeat_restored'
            and last_observed_at > firing_last_observed_at
            or
            resolution_reason = 'node_revoked'
            and last_observed_at = firing_last_observed_at
        )
    )
);

create index fleet_node_availability_fact_heads_due_idx
    on fleet_node_availability_fact_heads (
        timeout_deadline_at,
        organization_id,
        node_id
    )
    where state in ('observed', 'resolved');

create function enforce_fleet_node_availability_fact_head_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Fleet node availability fact heads cannot be deleted';
    end if;
    if new.organization_id is distinct from old.organization_id
       or new.node_id is distinct from old.node_id
       or new.node_aggregate_version < old.node_aggregate_version
       or new.last_observed_at < old.last_observed_at
       or new.updated_at < old.updated_at then
        raise exception 'Fleet node availability fact-head identity or cursor moved backwards';
    end if;

    if old.state in ('observed', 'resolved') and new.state = old.state then
        if new.latest_event_id is distinct from old.latest_event_id
           or new.latest_event_key is distinct from old.latest_event_key
           or new.latest_phase_version is distinct from old.latest_phase_version
           or new.firing_event_id is distinct from old.firing_event_id
           or new.firing_phase_version is distinct from old.firing_phase_version
           or new.firing_node_aggregate_version is distinct from old.firing_node_aggregate_version
           or new.firing_last_observed_at is distinct from old.firing_last_observed_at
           or new.firing_timeout_deadline_at is distinct from old.firing_timeout_deadline_at
           or new.detected_at is distinct from old.detected_at
           or new.resolved_at is distinct from old.resolved_at
           or new.resolution_reason is distinct from old.resolution_reason
           or new.last_observed_at > old.last_observed_at
              and new.timeout_deadline_at is not null
           or new.last_observed_at = old.last_observed_at
              and not (
                  old.timeout_deadline_at is null
                  and new.timeout_deadline_at is not null
              ) then
            raise exception 'Fleet node availability observation mutation is invalid';
        end if;
        return new;
    end if;

    if old.state in ('observed', 'resolved') and new.state = 'unavailable' then
        if new.last_observed_at is distinct from old.last_observed_at
           or new.latest_phase_version <= coalesce(old.latest_phase_version, 0)
           or new.detected_at <= new.timeout_deadline_at then
            raise exception 'Fleet node unavailable firing does not advance its observation head';
        end if;
        return new;
    end if;

    if old.state = 'unavailable' and new.state = 'resolved' then
        if new.firing_event_id is distinct from old.firing_event_id
           or new.firing_phase_version is distinct from old.firing_phase_version
           or new.firing_node_aggregate_version is distinct from old.firing_node_aggregate_version
           or new.firing_last_observed_at is distinct from old.firing_last_observed_at
           or new.firing_timeout_deadline_at is distinct from old.firing_timeout_deadline_at
           or new.detected_at is distinct from old.detected_at
           or new.timeout_deadline_at is not null
           or new.latest_phase_version <= old.latest_phase_version
           or new.node_aggregate_version <= old.node_aggregate_version then
            raise exception 'Fleet node availability resolution does not advance its firing';
        end if;
        return new;
    end if;

    raise exception 'Fleet node availability fact-head transition is invalid';
end
$$;

create trigger fleet_node_availability_fact_heads_transition_only
before update or delete on fleet_node_availability_fact_heads
for each row execute function enforce_fleet_node_availability_fact_head_transition();

comment on table fleet_node_availability_fact_heads is
    'Fleet-owned bounded per-Node owner-fact cursor; not a generic health, incident, metric, queue, scheduler, timer, log, inventory, command, credential, provider-response, or Notifications store';

comment on column fleet_node_availability_fact_heads.timeout_deadline_at is
    'Deadline anchored once per strictly advancing heartbeat so timeout-policy drift alone cannot create an availability fact';
