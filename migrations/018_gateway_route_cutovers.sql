create table gateway_route_cutovers (
    deployment_id uuid primary key references deployments(id),
    organization_id uuid not null,
    workload_id uuid not null,
    previous_revision_id uuid not null,
    candidate_revision_id uuid not null,
    node_id uuid not null,
    gateway_revision bigint not null check (gateway_revision > 0),
    gateway_command_id uuid not null,
    gateway_certificate_id uuid not null references gateway_certificates(id),
    snapshot_digest text not null check (snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    routes jsonb not null,
    state text not null check (state in ('pending', 'applied', 'rejected')),
    failure text,
    staged_at timestamptz not null,
    acknowledged_at timestamptz,
    unique (node_id, gateway_revision),
    foreign key (organization_id, workload_id)
        references workloads (organization_id, id),
    foreign key (workload_id, previous_revision_id)
        references workload_revisions (workload_id, id),
    foreign key (workload_id, candidate_revision_id)
        references workload_revisions (workload_id, id),
    foreign key (node_id, gateway_revision, gateway_command_id)
        references gateway_publications (node_id, revision, command_id),
    check (previous_revision_id <> candidate_revision_id),
    check (jsonb_typeof(routes) = 'array' and jsonb_array_length(routes) > 0),
    check (
        state = 'pending' and failure is null and acknowledged_at is null
        or state = 'applied' and failure is null and acknowledged_at is not null
        or state = 'rejected' and failure is not null and acknowledged_at is not null
    )
);

create index gateway_route_cutovers_workload_idx
    on gateway_route_cutovers (organization_id, workload_id, staged_at desc, deployment_id);
