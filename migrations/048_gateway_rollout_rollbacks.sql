create table gateway_rollout_rollbacks (
    failed_rollout_id uuid primary key
        references gateway_rollouts (id),
    gateway_scope_id uuid not null
        references gateway_route_scopes (id),
    membership_generation bigint not null
        check (membership_generation > 0),
    failed_generation bigint not null
        check (failed_generation > 0),
    rollback_rollout_id uuid not null unique,
    rollback_generation bigint not null
        check (rollback_generation > 0),
    state text not null
        check (state in ('required', 'staged', 'succeeded', 'diverged')),
    aggregate_version bigint not null
        check (aggregate_version > 0),
    required_at timestamptz not null,
    staged_at timestamptz,
    completed_at timestamptz,
    failure text,
    unique (gateway_scope_id, rollback_generation),
    check (rollback_generation = failed_generation + 1),
    check (staged_at is null or staged_at >= required_at),
    check (
        completed_at is null
        or completed_at >= coalesce(staged_at, required_at)
    ),
    check (
        state = 'required'
            and staged_at is null
            and completed_at is null
            and failure is null
        or state = 'staged'
            and staged_at is not null
            and completed_at is null
            and failure is null
        or state = 'succeeded'
            and staged_at is not null
            and completed_at is not null
            and failure is null
        or state = 'diverged'
            and completed_at is not null
            and failure is not null
    )
);

create unique index gateway_rollout_rollbacks_one_unresolved_scope_idx
    on gateway_rollout_rollbacks (gateway_scope_id)
    where state <> 'succeeded';

create index gateway_rollout_rollbacks_state_idx
    on gateway_rollout_rollbacks (
        state,
        required_at,
        failed_rollout_id
    );

create table gateway_route_ownership (
    gateway_rollout_id uuid not null,
    route_id uuid not null,
    gateway_node_id uuid not null,
    hostname text not null,
    path_prefix text not null,
    created_at timestamptz not null,
    primary key (gateway_node_id, hostname, path_prefix),
    unique (gateway_rollout_id, gateway_node_id),
    foreign key (gateway_rollout_id, gateway_node_id)
        references gateway_route_projections (
            gateway_rollout_id,
            gateway_node_id
        ),
    foreign key (route_id)
        references routes (id)
);

create index gateway_route_ownership_rollout_idx
    on gateway_route_ownership (
        gateway_rollout_id,
        gateway_node_id
    );

create index gateway_route_ownership_route_idx
    on gateway_route_ownership (
        route_id,
        gateway_node_id
    );

insert into gateway_route_ownership (
    gateway_rollout_id,
    route_id,
    gateway_node_id,
    hostname,
    path_prefix,
    created_at
)
select
    projection.gateway_rollout_id,
    projection.route_id,
    projection.gateway_node_id,
    projection.hostname,
    projection.path_prefix,
    projection.created_at
from gateway_route_projections projection
inner join routes logical_route
    on logical_route.id = projection.route_id
where logical_route.state in ('publishing', 'active')
    and projection.state in ('publishing', 'active', 'unavailable');

drop index gateway_route_projections_active_ownership_idx;
