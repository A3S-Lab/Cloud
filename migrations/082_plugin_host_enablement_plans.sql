alter table node_commands
    drop constraint node_commands_command_kind_check;

-- A legacy direct-enablement row cannot be reinterpreted as a reviewed plan.
-- Constraint validation therefore rejects such pre-release rows instead of
-- performing a lossy authority rewrite.
alter table node_commands
    add constraint node_commands_command_kind_check check (
        command_kind in (
            'runtime_apply',
            'runtime_inspect',
            'runtime_stop',
            'runtime_remove',
            'box_build_start',
            'box_build_inspect',
            'box_build_cancel',
            'box_build_remove',
            'gateway_snapshot_install',
            'gateway_snapshot_observe',
            'plugin_host_capabilities_inspect',
            'plugin_host_plan',
            'plugin_host_apply',
            'plugin_host_plan_enablement',
            'plugin_host_observe',
            'resource_claim_prepare',
            'resource_claim_release'
        )
    );
