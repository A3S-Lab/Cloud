do $$
begin
    if exists (
        select 1
        from resource_claims
        where state <> 'released'
        group by organization_id, workload_id, node_id
        having count(distinct replica_id) > 1
    ) then
        raise exception
            'required Workload replica anti-affinity has an existing active placement conflict';
    end if;
end;
$$;

alter table workload_controls
    drop constraint workload_controls_placement_policy_check;

with policy_values as (
    select
        workload_id,
        (placement_policy ->> 'generation')::bigint as generation,
        (placement_policy ->> 'desiredReplicas')::integer as desired_replicas,
        (placement_policy ->> 'membersPerReplica')::integer as members_per_replica,
        placement_policy ->> 'topology' as topology
    from workload_controls
),
upgraded as (
    select
        workload_id,
        generation + 1 as generation,
        desired_replicas,
        members_per_replica,
        topology,
        'sha256:' || encode(
            sha256(convert_to(
                '{"schema":"a3s.cloud.effective-placement-policy.v2","generation":'
                    || (generation + 1)::text
                    || ',"desiredReplicas":' || desired_replicas::text
                    || ',"membersPerReplica":' || members_per_replica::text
                    || ',"topology":"' || topology
                    || '","replicaAntiAffinity":"required"}',
                'UTF8'
            )),
            'hex'
        ) as digest
    from policy_values
)
update workload_controls as control
set placement_policy = jsonb_build_object(
        'schema', 'a3s.cloud.effective-placement-policy.v2',
        'generation', upgraded.generation,
        'desiredReplicas', upgraded.desired_replicas,
        'membersPerReplica', upgraded.members_per_replica,
        'topology', upgraded.topology,
        'replicaAntiAffinity', 'required',
        'digest', upgraded.digest
    ),
    placement_policy_digest = upgraded.digest,
    aggregate_version = control.aggregate_version + 1,
    updated_at = greatest(control.updated_at, transaction_timestamp())
from upgraded
where upgraded.workload_id = control.workload_id;

alter table workload_controls
    add constraint workload_controls_placement_policy_check check (
        jsonb_typeof(placement_policy) = 'object'
        and placement_policy ->> 'schema' =
            'a3s.cloud.effective-placement-policy.v2'
        and (placement_policy ->> 'generation')::bigint > 0
        and (placement_policy ->> 'desiredReplicas')::integer between 0 and 100
        and (placement_policy ->> 'membersPerReplica')::integer = 1
        and placement_policy ->> 'topology' = 'single_node'
        and placement_policy ->> 'replicaAntiAffinity' = 'required'
        and placement_policy ->> 'digest' = placement_policy_digest
        and placement_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    );

create index resource_claims_active_workload_node_replica_idx
    on resource_claims (organization_id, workload_id, node_id, replica_id)
    where state <> 'released';
