use super::*;

#[test]
fn agent_flow_reuses_code_protocol_without_owning_a_run_lifecycle() {
    let source = include_str!("runtime.rs");
    assert!(source.contains("NodeCommandPayload::CodeAgentCommand"));
    assert!(source.contains("AgentProtocolCommandV1::Start"));
    assert!(source.contains("AgentProtocolCommandV1::Cancel"));
    assert!(source.contains("a3s-code-cancel-v1"));
    assert!(source.contains("list_active_runtime_targets"));
    for forbidden in [
        "AgentSession",
        "AgentProtocolHost",
        "InMemoryRunStore",
        "spawn_run",
        "spawn_recovery",
        "cancel_run",
        "RunStore",
        "CreateAgentWorkloadDeployment",
        "CreateWorkloadDeployment",
        "create_deployment(",
        "RuntimeClient::apply",
    ] {
        assert!(
            !source.contains(forbidden),
            "Cloud Agent Flow must not own Code lifecycle primitive {forbidden}"
        );
    }
}

#[test]
fn agent_flow_configuration_is_bounded() {
    assert!(
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 1_000,
            command_ttl_ms: 10_000,
            observation_poll_ms: 1_000,
            convergence_timeout_ms: 60_000,
        })
        .is_ok()
    );
    assert!(
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 1_000,
            command_ttl_ms: 10_000,
            observation_poll_ms: 60_001,
            convergence_timeout_ms: 60_000,
        })
        .is_err()
    );
}
