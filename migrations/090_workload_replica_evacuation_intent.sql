alter table workload_replicas
    add column evacuation_node_id uuid;

alter table workload_replicas
    add constraint workload_replicas_evacuation_node_fk
        foreign key (organization_id, evacuation_node_id)
        references nodes (organization_id, id),
    add constraint workload_replicas_evacuation_state_check check (
        evacuation_node_id is null
        or lifecycle = 'retiring'
    );
