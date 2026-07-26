alter table gateway_publications
    drop constraint gateway_publications_state_check,
    drop constraint gateway_publications_check2,
    add constraint gateway_publications_state_check
        check (state in ('pending', 'applied', 'rejected', 'unavailable')),
    add constraint gateway_publications_check2
        check (
            state = 'pending' and failure is null and acknowledged_at is null
            or state = 'applied' and failure is null and acknowledged_at is not null
            or state in ('rejected', 'unavailable')
                and failure is not null
                and acknowledged_at is not null
        );

drop index routes_active_ownership_idx;

create unique index routes_active_ownership_idx
    on routes (gateway_scope_id, hostname, path_prefix)
    where state in ('publishing', 'active');

create table gateway_route_projections (
    gateway_rollout_id uuid not null,
    route_id uuid not null,
    gateway_scope_id uuid not null,
    membership_generation bigint not null check (membership_generation > 0),
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    gateway_node_id uuid not null,
    hostname text not null,
    path_prefix text not null,
    workload_id uuid not null,
    workload_revision_id uuid not null,
    runtime_unit_id text not null,
    runtime_generation bigint not null check (runtime_generation > 0),
    port_name text not null,
    upstream_origin text not null,
    target_observed_at timestamptz not null,
    state text not null check (
        state in ('publishing', 'active', 'rejected', 'unavailable')
    ),
    gateway_revision bigint not null check (gateway_revision > 0),
    gateway_command_id uuid not null,
    snapshot_digest text not null check (
        snapshot_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    failure text,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    activated_at timestamptz,
    domain_claim_id uuid references domain_claims(id),
    domain_pattern text,
    gateway_certificate_id uuid references gateway_certificates(id),
    primary key (gateway_rollout_id, gateway_node_id),
    foreign key (route_id)
        references routes (id),
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
    foreign key (gateway_rollout_id, gateway_node_id)
        references gateway_rollout_replicas (gateway_rollout_id, node_id),
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
    foreign key (gateway_scope_id, gateway_node_id)
        references gateway_scope_members (gateway_scope_id, node_id),
    foreign key (organization_id, workload_id)
        references workloads (organization_id, id),
    foreign key (
        workload_id,
        workload_revision_id,
        runtime_generation
    )
        references workload_revisions (workload_id, id, generation),
    foreign key (
        gateway_node_id,
        gateway_revision,
        gateway_command_id
    )
        references gateway_publications (node_id, revision, command_id),
    check (
        runtime_unit_id =
            'workload:' || workload_id::text
                || ':revision:' || workload_revision_id::text
    ),
    check (target_observed_at <= updated_at),
    check (updated_at >= created_at),
    check (
        domain_claim_id is null
            and domain_pattern is null
            and gateway_certificate_id is null
        or domain_claim_id is not null
            and domain_pattern is not null
            and gateway_certificate_id is not null
    ),
    check (
        state = 'publishing'
            and failure is null
            and activated_at is null
        or state = 'active'
            and failure is null
            and activated_at is not null
        or state = 'rejected'
            and failure is not null
            and activated_at is null
        or state = 'unavailable'
            and failure is not null
            and activated_at is null
    )
);

create unique index gateway_route_projections_active_ownership_idx
    on gateway_route_projections (gateway_node_id, hostname, path_prefix)
    where state in ('publishing', 'active', 'unavailable');

create index gateway_route_projections_route_idx
    on gateway_route_projections (
        route_id,
        gateway_rollout_id,
        gateway_node_id
    );

create index gateway_route_projections_node_state_idx
    on gateway_route_projections (
        gateway_node_id,
        state,
        hostname,
        path_prefix,
        route_id
    );
