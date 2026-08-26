alter table agent_executions
    drop constraint agent_executions_provider_binding_complete,
    drop constraint agent_executions_provider_binding_values;

-- Executions created before A1.3 selected the only admitted provider. Preserve
-- that immutable decision before making profile selection creation-time state.
update agent_executions
set provider_kind = 'a3s.code',
    provider_revision = '8.0.1',
    provider_protocol = 'a3s.cloud.agent-provider.v1',
    provider_native_protocol = 'a3s.code.agent.v1',
    provider_profile_acl = $profile$agent_provider "a3s.code" {
  capabilities = ["cancellation", "change_set", "cleanup", "event_pages", "recovery", "streaming_output"]
  native_protocol = "a3s.code.agent.v1"
  protocol = "a3s.cloud.agent-provider.v1"
  revision = "8.0.1"
  schema = "a3s.cloud.agent-provider-profile.v1"
}
$profile$,
    provider_profile_digest = 'sha256:7587d2e401ffe738c0416765ce3c5d1683ae853c533e10291b1b82029eabb926',
    provider_capability_digest = 'sha256:6a40014e842fc193722ac2cf2604532b3d88ec75132781f7c02edab55b8e976c'
where provider_kind is null;

alter table agent_executions
    alter column provider_kind set not null,
    alter column provider_revision set not null,
    alter column provider_protocol set not null,
    alter column provider_native_protocol set not null,
    alter column provider_profile_acl set not null,
    alter column provider_profile_digest set not null,
    alter column provider_capability_digest set not null;

alter table agent_executions
    add constraint agent_executions_provider_binding_complete check (
        (
            provider_node_id is null
            and provider_workload_id is null
            and provider_workload_revision_id is null
            and provider_deployment_id is null
            and provider_replica_id is null
            and provider_runtime_unit_id is null
            and provider_runtime_generation is null
            and provider_runtime_spec_digest is null
            and provider_service_port_name is null
            and provider_release_identity is null
            and provider_session_id is null
            and provider_run_id is null
            and provider_event_cursor is null
            and provider_state is null
            and provider_bound_at is null
            and provider_observed_at is null
        )
        or (
            provider_node_id is not null
            and provider_workload_id is not null
            and provider_workload_revision_id is not null
            and provider_deployment_id is not null
            and provider_replica_id is not null
            and provider_runtime_unit_id is not null
            and provider_runtime_generation is not null
            and provider_runtime_spec_digest is not null
            and provider_service_port_name is not null
            and provider_release_identity is not null
            and provider_session_id is not null
            and provider_run_id is not null
            and provider_state is not null
            and provider_bound_at is not null
        )
    ),
    add constraint agent_executions_provider_binding_values check (
        provider_kind ~ '^[a-z0-9]+([.-][a-z0-9]+)*$'
        and octet_length(provider_kind) <= 64
        and octet_length(provider_revision) between 1 and 128
        and provider_revision !~ E'[\r\n]'
        and provider_protocol = 'a3s.cloud.agent-provider.v1'
        and octet_length(provider_native_protocol) between 1 and 128
        and provider_native_protocol !~ E'[\r\n]'
        and octet_length(provider_profile_acl) between 1 and 16384
        and provider_profile_digest ~ '^sha256:[0-9a-f]{64}$'
        and provider_capability_digest ~ '^sha256:[0-9a-f]{64}$'
        and (provider_runtime_generation is null or provider_runtime_generation > 0)
        and (
            provider_runtime_unit_id is null
            or (
                octet_length(provider_runtime_unit_id) between 1 and 512
                and btrim(provider_runtime_unit_id) <> ''
                and position(chr(13) in provider_runtime_unit_id) = 0
                and position(chr(10) in provider_runtime_unit_id) = 0
            )
        )
        and (
            provider_runtime_spec_digest is null
            or provider_runtime_spec_digest ~ '^sha256:[0-9a-f]{64}$'
        )
        and (
            provider_service_port_name is null
            or (
                octet_length(provider_service_port_name) between 1 and 128
                and btrim(provider_service_port_name) <> ''
                and position(chr(13) in provider_service_port_name) = 0
                and position(chr(10) in provider_service_port_name) = 0
            )
        )
        and (
            provider_release_identity is null
            or (
                provider_release_identity ~ '^sha256:[0-9a-f]{64}$'
                and provider_release_identity = agent_artifact_digest
            )
        )
        and (
            provider_session_id is null
            or (
                octet_length(provider_session_id) between 1 and 256
                and btrim(provider_session_id) <> ''
                and provider_session_id !~ E'[\r\n]'
            )
        )
        and (
            provider_run_id is null
            or (
                octet_length(provider_run_id) between 1 and 256
                and btrim(provider_run_id) <> ''
                and provider_run_id !~ E'[\r\n]'
            )
        )
        and (provider_event_cursor is null or provider_event_cursor >= 0)
        and (
            provider_state is null
            or provider_state in (
                'created',
                'planning',
                'executing',
                'verifying',
                'completed',
                'failed',
                'cancelled'
            )
        )
        and (provider_bound_at is null or provider_bound_at >= requested_at)
    );

comment on column agent_executions.provider_kind is
    'A1.3 closed provider kind selected at execution creation; the canonical ACL and digests remain recovery authority';
