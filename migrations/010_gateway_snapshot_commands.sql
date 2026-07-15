alter table node_commands
    drop constraint node_commands_command_kind_check;

alter table node_commands
    add constraint node_commands_command_kind_check check (
        command_kind in (
            'runtime_apply',
            'runtime_inspect',
            'runtime_stop',
            'runtime_remove',
            'gateway_snapshot_install'
        )
    );

create unique index node_commands_one_gateway_revision_idx
    on node_commands (node_id, aggregate_id, generation)
    where command_kind = 'gateway_snapshot_install';

alter table node_gateway_acknowledgements
    add column command_id uuid;

create unique index node_commands_identity_idx
    on node_commands (id, node_id);

alter table node_gateway_acknowledgements
    add constraint node_gateway_acknowledgements_command_node_fk
    foreign key (command_id, node_id) references node_commands(id, node_id);

create unique index node_gateway_acknowledgements_command_idx
    on node_gateway_acknowledgements (node_id, command_id);
