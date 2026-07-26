alter table gateway_rollout_replicas
    add column recovery jsonb;

update gateway_rollout_replicas
set recovery = jsonb_build_object(
    'state', 'required',
    'attempt', 0,
    'failure', failure,
    'updated_at', acknowledged_at
)
where state = 'unavailable';

alter table gateway_rollout_replicas
    add constraint gateway_rollout_replicas_recovery_check
        check ((state = 'unavailable') = (recovery is not null));

create index gateway_rollout_replicas_recovery_idx
    on gateway_rollout_replicas (state, gateway_rollout_id, node_id)
    where recovery is not null;
