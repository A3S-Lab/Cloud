drop index node_commands_one_apply_generation_idx;

create index node_commands_apply_generation_idx
    on node_commands (node_id, aggregate_id, generation, sequence desc)
    where command_kind = 'runtime_apply';
