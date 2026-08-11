alter table workload_controls
    drop constraint workload_controls_check1;

alter table workload_controls
    add constraint workload_controls_placement_policy_check check (
        jsonb_typeof(placement_policy) = 'object'
        and placement_policy ->> 'schema' =
            'a3s.cloud.effective-placement-policy.v1'
        and (placement_policy ->> 'generation')::bigint > 0
        and (placement_policy ->> 'desiredReplicas')::integer between 0 and 100
        and (placement_policy ->> 'membersPerReplica')::integer = 1
        and placement_policy ->> 'topology' = 'single_node'
        and placement_policy ->> 'digest' = placement_policy_digest
        and placement_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    );

alter table workload_replicas
    add column revision_generation bigint,
    add column lifecycle text;

update workload_replicas
set revision_generation = generation,
    lifecycle = 'desired';

alter table workload_replicas
    alter column revision_generation set not null,
    alter column lifecycle set not null,
    drop constraint workload_replicas_workload_id_revision_id_generation_fkey,
    drop constraint workload_replicas_ordinal_check,
    drop constraint workload_replicas_check;

alter table workload_replicas
    add constraint workload_replicas_revision_fk
        foreign key (workload_id, revision_id, revision_generation)
        references workload_revisions (workload_id, id, generation) on delete cascade,
    add constraint workload_replicas_ordinal_check
        check (ordinal >= 0 and ordinal < 100),
    add constraint workload_replicas_revision_generation_check
        check (revision_generation > 0),
    add constraint workload_replicas_lifecycle_check
        check (lifecycle in ('desired', 'retiring', 'retired'));

alter table workload_replica_members
    drop constraint workload_replica_members_check,
    drop constraint workload_replica_members_check1;

alter table workload_replica_members
    add constraint workload_replica_members_identity_check
        check (ordinal = 0 and id = replica_id),
    add constraint workload_replica_members_placement_check
        check (node_id is null or placement_generation > 0);

alter table deployment_replica_bindings
    drop constraint deployment_replica_bindings_workload_id_revision_id_runtime_generation_fkey,
    drop constraint deployment_replica_bindings_check1;

alter table deployment_replica_bindings
    add constraint deployment_replica_bindings_revision_fk
        foreign key (workload_id, revision_id)
        references workload_revisions (workload_id, id);

alter table deployments
    drop constraint deployments_workload_id_revision_id_key;

create index workload_replicas_desired_idx
    on workload_replicas (organization_id, workload_id, lifecycle, ordinal);
