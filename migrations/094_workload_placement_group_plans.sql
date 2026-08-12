alter table workload_controls
    drop constraint workload_controls_placement_policy_check;

alter table workload_controls
    add constraint workload_controls_placement_policy_check check (
        jsonb_typeof(placement_policy) = 'object'
        and placement_policy ->> 'schema' =
            'a3s.cloud.effective-placement-policy.v3'
        and (placement_policy ->> 'generation')::bigint > 0
        and (placement_policy ->> 'desiredReplicas')::integer between 0 and 100
        and (placement_policy ->> 'membersPerReplica')::integer between 1 and 256
        and (
            (
                (placement_policy ->> 'membersPerReplica')::integer = 1
                and placement_policy ->> 'topology' = 'single_node'
            )
            or (
                (placement_policy ->> 'membersPerReplica')::integer between 2 and 256
                and placement_policy ->> 'topology' = 'multi_node'
            )
        )
        and placement_policy ->> 'replicaAntiAffinity' = 'required'
        and placement_policy ? 'nodePoolId'
        and jsonb_typeof(placement_policy -> 'nodePoolId') in ('null', 'string')
        and placement_policy ->> 'digest' = placement_policy_digest
        and placement_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    );

alter table workload_controls
    add column members_per_replica integer generated always as (
        (placement_policy ->> 'membersPerReplica')::integer
    ) stored,
    add column placement_topology text generated always as (
        placement_policy ->> 'topology'
    ) stored;

create index workload_controls_execution_shape_idx
    on workload_controls (
        organization_id,
        placement_topology,
        members_per_replica,
        workload_id
    );

alter table workload_replica_members
    drop constraint workload_replica_members_ordinal_check,
    drop constraint workload_replica_members_identity_check;

alter table workload_replica_members
    add constraint workload_replica_members_identity_check check (
        ordinal >= 0
        and ordinal < 256
        and (ordinal <> 0 or id = replica_id)
    );

create table workload_placement_groups (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    revision_id uuid not null,
    revision_generation bigint not null check (revision_generation > 0),
    replica_id uuid not null,
    replica_generation bigint not null check (replica_generation > 0),
    policy_generation bigint not null check (policy_generation > 0),
    placement_policy_digest text not null
        check (placement_policy_digest ~ '^sha256:[0-9a-f]{64}$'),
    plan_schema text not null,
    plan_digest text not null check (plan_digest ~ '^sha256:[0-9a-f]{64}$'),
    state text not null,
    member_count integer not null check (member_count between 2 and 256),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, id),
    unique (organization_id, replica_id, replica_generation),
    foreign key (organization_id, workload_id, replica_id)
        references workload_replicas (organization_id, workload_id, id) on delete cascade,
    foreign key (workload_id, revision_id, revision_generation)
        references workload_revisions (workload_id, id, generation),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (plan_schema = 'a3s.cloud.workload-placement-group-plan.v1'),
    check (state = 'planned'),
    check (updated_at >= created_at)
);

create table workload_placement_group_members (
    organization_id uuid not null,
    group_id uuid not null,
    workload_id uuid not null,
    replica_id uuid not null,
    member_id uuid not null,
    ordinal integer not null check (ordinal >= 0 and ordinal < 256),
    role text not null,
    runtime_unit_id text not null,
    template jsonb not null,
    template_digest text not null
        check (template_digest ~ '^sha256:[0-9a-f]{64}$'),
    primary key (group_id, member_id),
    unique (group_id, ordinal),
    unique (group_id, runtime_unit_id),
    foreign key (organization_id, group_id)
        references workload_placement_groups (organization_id, id) on delete cascade,
    foreign key (workload_id, replica_id, member_id)
        references workload_replica_members (workload_id, replica_id, id),
    check (
        (ordinal = 0 and role = 'leader')
        or (ordinal > 0 and role = 'worker')
    ),
    check (jsonb_typeof(template) = 'object'),
    check (
        length(runtime_unit_id) between 1 and 512
        and runtime_unit_id !~ '[\x00\r\n]'
    )
);

create index workload_placement_groups_reconcile_idx
    on workload_placement_groups (organization_id, state, updated_at, id);

create index workload_placement_group_members_replica_idx
    on workload_placement_group_members (organization_id, replica_id, ordinal, member_id);
