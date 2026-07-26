create table gateway_rollouts (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    gateway_scope_id uuid not null,
    membership_generation bigint not null check (membership_generation > 0),
    generation bigint not null check (generation > 0),
    correlation_id uuid not null,
    min_ready integer not null check (min_ready > 0 and min_ready <= 100),
    max_unavailable integer not null check (
        max_unavailable >= 0 and max_unavailable < 100
    ),
    desired_replicas integer not null check (
        desired_replicas > 0 and desired_replicas <= 100
    ),
    state text not null check (
        state in ('pending', 'ready', 'succeeded', 'degraded')
    ),
    ready_replicas integer not null check (ready_replicas >= 0),
    unavailable_replicas integer not null check (unavailable_replicas >= 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    started_at timestamptz not null,
    completed_at timestamptz,
    unique (gateway_scope_id, generation),
    unique (id, gateway_scope_id, membership_generation),
    foreign key (
        gateway_scope_id,
        organization_id,
        project_id,
        environment_id
    )
        references gateway_route_scopes (
            id,
            organization_id,
            project_id,
            environment_id
        ),
    check (min_ready <= desired_replicas),
    check (max_unavailable < desired_replicas),
    check (ready_replicas + unavailable_replicas <= desired_replicas),
    check (
        state = 'pending'
            and completed_at is null
            and ready_replicas + unavailable_replicas < desired_replicas
            and (
                ready_replicas < min_ready
                or desired_replicas - ready_replicas > max_unavailable
            )
        or state = 'ready'
            and completed_at is null
            and ready_replicas + unavailable_replicas < desired_replicas
            and ready_replicas < desired_replicas
            and ready_replicas >= min_ready
            and desired_replicas - ready_replicas <= max_unavailable
        or state = 'succeeded'
            and completed_at is not null
            and ready_replicas = desired_replicas
            and unavailable_replicas = 0
        or state = 'degraded'
            and completed_at is not null
            and unavailable_replicas > 0
            and ready_replicas + unavailable_replicas = desired_replicas
    ),
    check (completed_at is null or completed_at >= started_at)
);

create unique index gateway_rollouts_one_active_idx
    on gateway_rollouts (gateway_scope_id)
    where state in ('pending', 'ready');

create index gateway_rollouts_environment_idx
    on gateway_rollouts (
        organization_id,
        project_id,
        environment_id,
        started_at desc,
        id
    );

create table gateway_rollout_replicas (
    gateway_rollout_id uuid not null,
    gateway_scope_id uuid not null,
    membership_generation bigint not null check (membership_generation > 0),
    node_id uuid not null,
    revision bigint not null check (revision > 0),
    command_id uuid not null,
    snapshot_digest text not null check (
        snapshot_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    snapshot_expires_at timestamptz not null,
    gateway_certificate_id uuid references gateway_certificates(id),
    state text not null check (
        state in ('pending', 'applied', 'rejected', 'unavailable')
    ),
    failure text,
    acknowledged_at timestamptz,
    primary key (gateway_rollout_id, node_id),
    unique (node_id, revision),
    foreign key (
        gateway_rollout_id,
        gateway_scope_id,
        membership_generation
    )
        references gateway_rollouts (
            id,
            gateway_scope_id,
            membership_generation
        ),
    foreign key (gateway_scope_id, node_id)
        references gateway_scope_members (gateway_scope_id, node_id),
    foreign key (node_id, revision, command_id)
        references gateway_publications (node_id, revision, command_id),
    check (
        state = 'pending'
            and failure is null
            and acknowledged_at is null
        or state = 'applied'
            and failure is null
            and acknowledged_at is not null
        or state in ('rejected', 'unavailable')
            and failure is not null
            and acknowledged_at is not null
    )
);

create index gateway_rollout_replicas_publication_idx
    on gateway_rollout_replicas (node_id, command_id, gateway_rollout_id);
