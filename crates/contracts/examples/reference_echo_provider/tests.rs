use super::*;
use a3s_cloud_contracts::{
    AgentProviderApprovalDecisionV1, AgentProviderApprovalOutcomeV1, AgentProviderCapabilityV1,
    AgentProviderRunResumeV1, HarnessAgentReleaseBindingV1, HarnessInvocationProfileV1,
    HarnessProviderBindingV1, HarnessWorkspaceBindingV1,
};
use uuid::Uuid;

#[test]
fn approval_resume_requires_the_exact_pending_tool_identity_and_replays() {
    let mut state = FixtureState::new().expect("reference state");
    let profile = state.profile.clone();
    let tool = HarnessToolBindingV1 {
        name: "workspace.publish".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "e".repeat(64)),
        approval_required: true,
    };
    let invocation = HarnessInvocationProfileV1 {
        schema: HarnessInvocationProfileV1::SCHEMA.into(),
        agent: HarnessAgentReleaseBindingV1 {
            organization_id: Uuid::from_u128(1),
            asset_id: Uuid::from_u128(2),
            asset_release_id: Uuid::from_u128(3),
            build_run_id: Uuid::from_u128(4),
            artifact_digest: format!("sha256:{}", "a".repeat(64)),
        },
        provider: HarnessProviderBindingV1 {
            kind: profile.kind().into(),
            revision: profile.revision().into(),
            profile_digest: profile.digest().into(),
            capability_digest: profile.capability_digest().into(),
        },
        instructions_digest: format!("sha256:{}", "a".repeat(64)),
        environment_policy_digest: format!("sha256:{}", "b".repeat(64)),
        security_policy_digest: format!("sha256:{}", "c".repeat(64)),
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: Uuid::from_u128(5),
            workload_revision_id: Uuid::from_u128(6),
            runtime_unit_id: "reference-provider".into(),
            runtime_generation: 1,
            runtime_spec_digest: format!("sha256:{}", "d".repeat(64)),
            working_directory: Some("/".into()),
        },
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        models: Vec::new(),
        secrets: Vec::new(),
        tools: vec![tool.clone()],
        required_capabilities: vec![
            AgentProviderCapabilityV1::Cancellation,
            AgentProviderCapabilityV1::Cleanup,
            AgentProviderCapabilityV1::EventPages,
            AgentProviderCapabilityV1::PauseResume,
            AgentProviderCapabilityV1::ToolCalls,
        ],
    };
    invocation
        .validate_for(&profile)
        .expect("approval invocation");
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        invocation.agent.artifact_digest.clone(),
        "reference-approval-conversation".into(),
        "reference-approval-run".into(),
    )
    .expect("approval identity");
    let start_request = AgentProviderRunStartV1::new_with_invocation_profile(
        "reference-approval-start".into(),
        identity,
        invocation,
        APPROVAL_PROMPT.into(),
    )
    .expect("approval start");
    let identity = start_request.identity.clone();
    let start = AgentProviderCommandV1::Start {
        request: start_request,
    };
    let started = state.accept_command(start).expect("accepted start");
    assert_eq!(started.state, AgentProviderRunStateV1::AwaitingApproval);

    let pending = state
        .runs
        .get(&identity.run_id)
        .and_then(|run| run.pending_approval.clone())
        .expect("pending Tool approval");
    let changed_decision =
        approval_decision(&identity, &pending, format!("sha256:{}", "0".repeat(64)));
    let changed_resume = AgentProviderCommandV1::Resume {
        request: AgentProviderRunResumeV1::new(
            "reference-approval-resume-changed".into(),
            identity.clone(),
            changed_decision,
        )
        .expect("changed resume contract"),
    };
    assert!(state.accept_command(changed_resume).is_err());
    let retained = state.runs.get(&identity.run_id).expect("retained run");
    assert_eq!(retained.state, AgentProviderRunStateV1::AwaitingApproval);
    assert!(retained.pending_approval.is_some());

    let exact_decision = approval_decision(&identity, &pending, pending.request_digest.clone());
    let exact_resume = AgentProviderCommandV1::Resume {
        request: AgentProviderRunResumeV1::new(
            "reference-approval-resume-exact".into(),
            identity.clone(),
            exact_decision,
        )
        .expect("exact resume contract"),
    };
    let resumed = state
        .accept_command(exact_resume.clone())
        .expect("accepted exact resume");
    assert_eq!(resumed.state, AgentProviderRunStateV1::Executing);
    assert!(!resumed.replayed);
    assert!(state
        .runs
        .get(&identity.run_id)
        .expect("resumed run")
        .pending_approval
        .is_none());

    let replayed = state
        .accept_command(exact_resume)
        .expect("replayed exact resume");
    assert!(replayed.replayed);
    assert_eq!(replayed.command_digest, resumed.command_digest);
    assert_eq!(replayed.state, resumed.state);
}

fn approval_decision(
    identity: &AgentProviderRunIdentityV1,
    pending: &PendingApproval,
    request_digest: String,
) -> AgentProviderApprovalDecisionV1 {
    AgentProviderApprovalDecisionV1::new(
        format!("reference-decision-{}", Uuid::now_v7()),
        "reference-checkpoint".into(),
        identity,
        pending.call_id.clone(),
        pending.tool.clone(),
        request_digest,
        AgentProviderApprovalOutcomeV1::Approved,
        1,
    )
    .expect("approval decision")
}
