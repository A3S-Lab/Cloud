alter table node_commands
    drop constraint node_commands_command_kind_check;

alter table node_commands
    add constraint node_commands_command_kind_check check (
        command_kind in (
            'runtime_apply',
            'runtime_inspect',
            'runtime_stop',
            'runtime_remove',
            'gateway_snapshot_install',
            'resource_claim_prepare',
            'resource_claim_release'
        )
    );
