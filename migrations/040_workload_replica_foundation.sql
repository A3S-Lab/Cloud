create table workload_controls (
    workload_id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    managed_owner_kind text,
    managed_owner_id uuid,
    managed_owner_generation bigint,
    managed_owner_spec_digest text,
    placement_policy jsonb not null,
    placement_policy_digest text not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, workload_id),
    foreign key (organization_id, workload_id)
        references workloads (organization_id, id) on delete cascade,
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (
        (
            managed_owner_kind is null
            and managed_owner_id is null
            and managed_owner_generation is null
            and managed_owner_spec_digest is null
        )
        or (
            managed_owner_kind ~ '^[a-z][a-z0-9-]{0,31}(\.[a-z][a-z0-9-]{0,31})+$'
            and length(managed_owner_kind) <= 64
            and managed_owner_id is not null
            and managed_owner_generation > 0
            and managed_owner_spec_digest ~ '^sha256:[0-9a-f]{64}$'
        )
    ),
    check (
        jsonb_typeof(placement_policy) = 'object'
        and placement_policy ->> 'schema' =
            'a3s.cloud.effective-placement-policy.v1'
        and (placement_policy ->> 'generation')::bigint > 0
        and (placement_policy ->> 'desiredReplicas')::integer = 1
        and (placement_policy ->> 'membersPerReplica')::integer = 1
        and placement_policy ->> 'topology' = 'single_node'
        and placement_policy ->> 'digest' = placement_policy_digest
        and placement_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    check (updated_at >= created_at)
);

create table workload_replicas (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    ordinal integer not null,
    revision_id uuid not null,
    generation bigint not null check (generation > 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, workload_id, id),
    unique (workload_id, ordinal),
    unique (workload_id, id, generation),
    foreign key (organization_id, workload_id)
        references workload_controls (organization_id, workload_id) on delete cascade,
    foreign key (workload_id, revision_id, generation)
        references workload_revisions (workload_id, id, generation) on delete cascade,
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (ordinal = 0),
    check (id = workload_id),
    check (updated_at >= created_at)
);

create table workload_replica_members (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    replica_id uuid not null,
    ordinal integer not null,
    node_id uuid,
    placement_generation bigint not null check (placement_generation >= 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, replica_id, id),
    unique (replica_id, ordinal),
    unique (workload_id, replica_id, id),
    foreign key (organization_id, workload_id, replica_id)
        references workload_replicas (organization_id, workload_id, id) on delete cascade,
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (ordinal = 0),
    check (id = workload_id),
    check ((node_id is null) = (placement_generation = 0)),
    check (updated_at >= created_at)
);

create table deployment_replica_bindings (
    deployment_id uuid primary key references deployments(id) on delete cascade,
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
    unique (replica_id, replica_generation),
    foreign key (organization_id, workload_id, replica_id)
        references workload_replicas (organization_id, workload_id, id),
    foreign key (workload_id, replica_id, member_id)
        references workload_replica_members (workload_id, replica_id, id),
    foreign key (workload_id, revision_id, runtime_generation)
        references workload_revisions (workload_id, id, generation),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (revision_id is not null),
    check (replica_generation = runtime_generation),
    check (
        runtime_unit_id =
            'workload:' || workload_id::text
                || ':revision:' || revision_id::text
    ),
    check (node_id is null or placement_generation > 0),
    check (updated_at >= created_at)
);

insert into workload_controls (
    workload_id,
    organization_id,
    project_id,
    environment_id,
    placement_policy,
    placement_policy_digest,
    aggregate_version,
    created_at,
    updated_at
)
select
    workload.id,
    workload.organization_id,
    workload.project_id,
    workload.environment_id,
    jsonb_build_object(
        'schema', 'a3s.cloud.effective-placement-policy.v1',
        'generation', 1,
        'desiredReplicas', 1,
        'membersPerReplica', 1,
        'topology', 'single_node',
        'digest', 'sha256:85b6b2c4115ff1822a689e8b84fccea7ff481ce606e47e9d0f237a57e2f032e1'
    ),
    'sha256:85b6b2c4115ff1822a689e8b84fccea7ff481ce606e47e9d0f237a57e2f032e1',
    1,
    workload.created_at,
    workload.updated_at
from workloads workload;

insert into workload_replicas (
    id,
    organization_id,
    project_id,
    environment_id,
    workload_id,
    ordinal,
    revision_id,
    generation,
    aggregate_version,
    created_at,
    updated_at
)
select
    workload.id,
    workload.organization_id,
    workload.project_id,
    workload.environment_id,
    workload.id,
    0,
    revision.id,
    revision.generation,
    revision.generation,
    workload.created_at,
    greatest(workload.updated_at, revision.created_at)
from workloads workload
join lateral (
    select candidate.id, candidate.generation, candidate.created_at
    from workload_revisions candidate
    where candidate.workload_id = workload.id
    order by candidate.generation desc, candidate.id desc
    limit 1
) revision on true;

insert into workload_replica_members (
    id,
    organization_id,
    project_id,
    environment_id,
    workload_id,
    replica_id,
    ordinal,
    node_id,
    placement_generation,
    aggregate_version,
    created_at,
    updated_at
)
select
    workload.id,
    workload.organization_id,
    workload.project_id,
    workload.environment_id,
    workload.id,
    workload.id,
    0,
    placement.node_id,
    case when placement.node_id is null then 0 else 1 end,
    case when placement.node_id is null then 1 else 2 end,
    workload.created_at,
    greatest(workload.updated_at, coalesce(placement.updated_at, workload.created_at))
from workloads workload
left join lateral (
    select deployment.node_id, deployment.updated_at
    from deployments deployment
    join workload_revisions revision
        on revision.workload_id = deployment.workload_id
        and revision.id = deployment.revision_id
    where deployment.workload_id = workload.id
        and deployment.node_id is not null
    order by revision.generation desc, deployment.id desc
    limit 1
) placement on true;

insert into deployment_replica_bindings (
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
    deployment.id,
    deployment.organization_id,
    workload.project_id,
    workload.environment_id,
    deployment.workload_id,
    deployment.revision_id,
    deployment.workload_id,
    revision.generation,
    deployment.workload_id,
    deployment.node_id,
    member.placement_generation,
    'workload:' || deployment.workload_id::text
        || ':revision:' || deployment.revision_id::text,
    revision.generation,
    deployment.requested_at,
    deployment.updated_at
from deployments deployment
join workloads workload
    on workload.organization_id = deployment.organization_id
    and workload.id = deployment.workload_id
join workload_revisions revision
    on revision.workload_id = deployment.workload_id
    and revision.id = deployment.revision_id
join workload_replica_members member
    on member.workload_id = deployment.workload_id;

create index workload_replicas_reconcile_idx
    on workload_replicas (organization_id, updated_at, id);

create index deployment_replica_bindings_runtime_idx
    on deployment_replica_bindings (
        organization_id,
        runtime_unit_id,
        runtime_generation
    );
