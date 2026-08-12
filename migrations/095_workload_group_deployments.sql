create table deployment_replica_member_bindings (
    deployment_id uuid not null references deployments(id) on delete cascade,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    revision_id uuid not null,
    replica_id uuid not null,
    replica_generation bigint not null check (replica_generation > 0),
    member_id uuid not null,
    node_id uuid,
    placement_generation bigint not null check (placement_generation >= 0),
    runtime_unit_id text not null,
    runtime_generation bigint not null check (runtime_generation > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (deployment_id, member_id),
    unique (organization_id, deployment_id, member_id),
    unique (replica_id, replica_generation, member_id),
    foreign key (organization_id, workload_id, replica_id)
        references workload_replicas (organization_id, workload_id, id),
    foreign key (workload_id, replica_id, member_id)
        references workload_replica_members (workload_id, replica_id, id),
    foreign key (workload_id, revision_id)
        references workload_revisions (workload_id, id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (replica_generation = runtime_generation),
    check (
        length(runtime_unit_id) between 1 and 512
        and runtime_unit_id !~ '[\x00\r\n]'
    ),
    check (node_id is null or placement_generation > 0),
    check (updated_at >= created_at)
);

insert into deployment_replica_member_bindings (
    deployment_id,
    organization_id,
    project_id,
    environment_id,
    workload_id,
    revision_id,
    replica_id,
    replica_generation,
    member_id,
    node_id,
    placement_generation,
    runtime_unit_id,
    runtime_generation,
    created_at,
    updated_at
)
select
    deployment_id,
    organization_id,
    project_id,
    environment_id,
    workload_id,
    revision_id,
    replica_id,
    replica_generation,
    member_id,
    node_id,
    placement_generation,
    runtime_unit_id,
    runtime_generation,
    created_at,
    updated_at
from deployment_replica_bindings;

alter table resource_claims
    add constraint resource_claims_deployment_member_fk
        foreign key (deployment_id, member_id)
        references deployment_replica_member_bindings (deployment_id, member_id);

create table deployment_placement_group_bindings (
    deployment_id uuid primary key
        references deployment_replica_bindings(deployment_id) on delete cascade,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    revision_id uuid not null,
    revision_generation bigint not null check (revision_generation > 0),
    replica_id uuid not null,
    replica_generation bigint not null check (replica_generation > 0),
    group_id uuid not null,
    group_plan_digest text not null
        check (group_plan_digest ~ '^sha256:[0-9a-f]{64}$'),
    member_count integer not null check (member_count between 2 and 256),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, deployment_id),
    unique (organization_id, group_id),
    unique (organization_id, replica_id, replica_generation),
    foreign key (organization_id, group_id)
        references workload_placement_groups (organization_id, id),
    foreign key (organization_id, workload_id, replica_id)
        references workload_replicas (organization_id, workload_id, id),
    foreign key (workload_id, revision_id, revision_generation)
        references workload_revisions (workload_id, id, generation),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (updated_at >= created_at)
);

create index deployment_replica_member_bindings_runtime_idx
    on deployment_replica_member_bindings (
        organization_id,
        runtime_unit_id,
        runtime_generation
    );

create index deployment_placement_group_bindings_reconcile_idx
    on deployment_placement_group_bindings (
        organization_id,
        updated_at,
        deployment_id
    );
