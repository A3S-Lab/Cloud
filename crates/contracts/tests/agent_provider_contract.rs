use a3s_cloud_contracts::{
    AgentProviderApprovalDecisionV1, AgentProviderApprovalOutcomeV1,
    AgentProviderCapabilityRequirementsV1, AgentProviderCapabilityV1,
    AgentProviderCommandReceiptV1, AgentProviderCommandV1, AgentProviderEventPageRequestV1,
    AgentProviderEventPageV1, AgentProviderEventReceiptV1, AgentProviderEventRecordV1,
    AgentProviderProfile, AgentProviderRunIdentityV1, AgentProviderRunResumeV1,
    AgentProviderRunStartV1, AgentProviderRunStateV1, AgentProviderSemanticEventV1,
    AgentProviderToolPayloadIdentityV1, AgentProviderToolResultOutcomeV1,
    HarnessAgentReleaseBindingV1, HarnessInvocationProfileV1, HarnessProviderBindingV1,
    HarnessSecretReferenceV1, HarnessSecretTargetV1, HarnessSkillBindingV1, HarnessToolBindingV1,
    HarnessWorkspaceBindingV1, AGENT_PROVIDER_MAX_EVENTS_PER_PAGE, AGENT_PROVIDER_PROTOCOL_V1,
};

const CODE_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/a3s-code-provider-profile.acl"
));
const REFERENCE_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));
const TOOL_PROFILE: &str = r#"agent_provider "test.tools" {
  capabilities = ["cancellation", "cleanup", "event_pages", "pause_resume", "tool_calls"]
  native_protocol = "test.tools.v1"
  protocol = "a3s.cloud.agent-provider.v1"
  revision = "1.0.0"
  schema = "a3s.cloud.agent-provider-profile.v1"
}
"#;
const TOOL_ONLY_PROFILE: &str = r#"agent_provider "test.tools-only" {
  capabilities = ["cancellation", "cleanup", "event_pages", "tool_calls"]
  native_protocol = "test.tools-only.v1"
  protocol = "a3s.cloud.agent-provider.v1"
  revision = "1.0.0"
  schema = "a3s.cloud.agent-provider-profile.v1"
}
"#;

fn invocation_profile(provider: &AgentProviderProfile) -> HarnessInvocationProfileV1 {
    HarnessInvocationProfileV1 {
        schema: HarnessInvocationProfileV1::SCHEMA.into(),
        agent: HarnessAgentReleaseBindingV1 {
            organization_id: uuid::Uuid::from_u128(1),
            asset_id: uuid::Uuid::from_u128(2),
            asset_release_id: uuid::Uuid::from_u128(3),
            build_run_id: uuid::Uuid::from_u128(4),
            artifact_digest: format!("sha256:{}", "a".repeat(64)),
        },
        provider: HarnessProviderBindingV1 {
            kind: provider.kind().into(),
            revision: provider.revision().into(),
            profile_digest: provider.digest().into(),
            capability_digest: provider.capability_digest().into(),
        },
        instructions_digest: format!("sha256:{}", "a".repeat(64)),
        environment_policy_digest: format!("sha256:{}", "b".repeat(64)),
        security_policy_digest: format!("sha256:{}", "c".repeat(64)),
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: uuid::Uuid::from_u128(5),
            workload_revision_id: uuid::Uuid::from_u128(6),
            runtime_unit_id: "workload:5:revision:6".into(),
            runtime_generation: 1,
            runtime_spec_digest: format!("sha256:{}", "d".repeat(64)),
            working_directory: Some("/workspace".into()),
        },
        skills: vec![HarnessSkillBindingV1 {
            asset_id: uuid::Uuid::from_u128(7),
            asset_release_id: uuid::Uuid::from_u128(8),
            artifact_digest: format!("sha256:{}", "e".repeat(64)),
        }],
        mcp_servers: Vec::new(),
        models: Vec::new(),
        secrets: vec![HarnessSecretReferenceV1 {
            name: "api-token".into(),
            secret_id: uuid::Uuid::from_u128(9),
            version: 3,
            target: HarnessSecretTargetV1::Environment {
                variable: "API_TOKEN".into(),
            },
        }],
        tools: Vec::new(),
        required_capabilities: vec![
            AgentProviderCapabilityV1::Cancellation,
            AgentProviderCapabilityV1::Cleanup,
            AgentProviderCapabilityV1::EventPages,
        ],
    }
}

#[test]
fn immutable_profiles_bind_canonical_acl_and_capabilities() {
    let code = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("Code provider profile");
    let reference =
        AgentProviderProfile::parse_acl(REFERENCE_PROFILE).expect("reference provider profile");

    assert_eq!(code.kind(), "a3s.code");
    assert_eq!(code.revision(), "8.2.0");
    assert_eq!(code.protocol(), AGENT_PROVIDER_PROTOCOL_V1);
    assert_eq!(code.native_protocol(), "a3s.code.agent.v1");
    assert_eq!(code.canonical_acl(), CODE_PROFILE);
    assert_ne!(code.digest(), reference.digest());
    assert_ne!(code.capability_digest(), reference.capability_digest());
    AgentProviderProfile::restore(code.canonical_acl(), code.digest())
        .expect("restore exact immutable profile");
    assert!(AgentProviderProfile::restore(code.canonical_acl(), reference.digest()).is_err());
}

#[test]
fn capability_negotiation_fails_closed() {
    let reference =
        AgentProviderProfile::parse_acl(REFERENCE_PROFILE).expect("reference provider profile");
    let baseline = AgentProviderCapabilityRequirementsV1::new(vec![
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::EventPages,
    ])
    .expect("baseline requirements");
    let negotiation = reference
        .negotiate(&baseline)
        .expect("supported baseline capabilities");
    negotiation
        .validate_for(&reference, &baseline)
        .expect("bound negotiation evidence");

    let checkpoint =
        AgentProviderCapabilityRequirementsV1::new(vec![AgentProviderCapabilityV1::Checkpoints])
            .expect("checkpoint requirements");
    let error = reference
        .negotiate(&checkpoint)
        .expect_err("unsupported capability must fail closed");
    assert!(error.contains("checkpoints"));
}

#[test]
fn versioned_commands_and_receipts_are_profile_bound() {
    let profile = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("provider profile");
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().to_owned(),
        profile.capability_digest().to_owned(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-1".into(),
    )
    .expect("run identity");
    let identity_digest = identity.digest().expect("run identity digest");
    assert_eq!(
        identity_digest,
        identity
            .clone()
            .digest()
            .expect("stable run identity digest")
    );
    let command = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "execution-1-start".into(),
            identity,
            "Summarize the release evidence".into(),
        )
        .expect("start request"),
    };
    command
        .validate_for(&profile)
        .expect("profile-bound command");
    let receipt = AgentProviderCommandReceiptV1::accepted(
        &profile,
        &command,
        AgentProviderRunStateV1::Created,
        1,
        false,
    )
    .expect("receipt");
    receipt
        .validate_for(&profile, &command)
        .expect("profile-bound receipt");

    let other = AgentProviderProfile::parse_acl(REFERENCE_PROFILE).expect("other profile");
    assert!(receipt.validate_for(&other, &command).is_err());
}

#[test]
fn invocation_profiles_are_closed_digest_bound_and_carried_by_profile_starts() {
    let provider = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("provider profile");
    let invocation = invocation_profile(&provider);
    invocation
        .validate_for(&provider)
        .expect("Harness invocation profile");
    let digest = invocation.digest().expect("invocation digest");
    assert_eq!(digest, invocation.digest().expect("stable digest"));

    let identity = AgentProviderRunIdentityV1::new(
        provider.digest().into(),
        provider.capability_digest().into(),
        invocation.agent.artifact_digest.clone(),
        "conversation-1".into(),
        "execution-1".into(),
    )
    .expect("run identity");
    let command = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new_with_invocation_profile(
            "execution-1-start".into(),
            identity,
            invocation.clone(),
            "Run the exact profile".into(),
        )
        .expect("profile-bound start"),
    };
    command
        .validate_for(&provider)
        .expect("profile-bound provider command");
    assert_eq!(
        command.identity().invocation_profile_digest.as_deref(),
        Some(digest.as_str())
    );

    let mismatched_identity = AgentProviderRunIdentityV1::new(
        provider.digest().into(),
        provider.capability_digest().into(),
        format!("sha256:{}", "f".repeat(64)),
        "conversation-1".into(),
        "execution-2".into(),
    )
    .expect("mismatched run identity");
    assert!(AgentProviderRunStartV1::new_with_invocation_profile(
        "execution-2-start".into(),
        mismatched_identity,
        invocation.clone(),
        "Run the exact profile".into(),
    )
    .is_err());

    let mut unsafe_number = invocation.clone();
    unsafe_number.workspace.runtime_generation = 9_007_199_254_740_992;
    assert!(unsafe_number.validate().is_err());

    let mut duplicate_secret_target = invocation.clone();
    duplicate_secret_target
        .secrets
        .push(HarnessSecretReferenceV1 {
            name: "other-token".into(),
            secret_id: uuid::Uuid::from_u128(10),
            version: 1,
            target: HarnessSecretTargetV1::Environment {
                variable: "API_TOKEN".into(),
            },
        });
    assert!(duplicate_secret_target.validate().is_err());

    let mut changed = invocation;
    changed.workspace.runtime_generation = 2;
    let mut encoded = serde_json::to_value(&changed).expect("profile JSON");
    encoded
        .as_object_mut()
        .expect("profile object")
        .insert("mutableProviderConfig".into(), serde_json::json!({}));
    assert!(serde_json::from_value::<HarnessInvocationProfileV1>(encoded).is_err());
}

#[test]
fn event_page_requests_are_profile_bound_and_use_the_public_limit() {
    let profile = AgentProviderProfile::parse_acl(REFERENCE_PROFILE).expect("provider profile");
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().to_owned(),
        profile.capability_digest().to_owned(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-1".into(),
    )
    .expect("run identity");
    let mut request = AgentProviderEventPageRequestV1 {
        schema: AgentProviderEventPageRequestV1::SCHEMA.into(),
        identity,
        after_event_sequence: Some(7),
        limit: u16::try_from(AGENT_PROVIDER_MAX_EVENTS_PER_PAGE)
            .expect("public event-page limit fits the protocol field"),
    };
    request
        .validate_for(&profile)
        .expect("profile-bound event-page request");

    request.limit = 0;
    assert!(request.validate_for(&profile).is_err());
    request.limit = u16::try_from(AGENT_PROVIDER_MAX_EVENTS_PER_PAGE + 1)
        .expect("invalid event-page limit fits the protocol field");
    assert!(request.validate_for(&profile).is_err());

    let other = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("other profile");
    request.limit = 1;
    assert!(request.validate_for(&other).is_err());
}

#[test]
fn profiles_reject_json_and_noncanonical_acl() {
    assert!(AgentProviderProfile::parse_acl(r#"{"kind":"a3s.code"}"#).is_err());
    assert!(
        AgentProviderProfile::parse_acl(&CODE_PROFILE.replace("  schema", "    schema")).is_err()
    );
    assert!(AgentProviderCapabilityRequirementsV1::new(vec![
        AgentProviderCapabilityV1::EventPages,
        AgentProviderCapabilityV1::Cancellation,
    ])
    .is_err());
}

#[test]
fn event_pages_and_duplicate_receipts_preserve_one_exact_sequence() {
    let profile = AgentProviderProfile::parse_acl(REFERENCE_PROFILE).expect("provider profile");
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().to_owned(),
        profile.capability_digest().to_owned(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-1".into(),
    )
    .expect("run identity");
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity,
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(0),
        source_last_sequence: Some(0),
        source_event_count: 1,
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProviderRunStateV1::Completed,
        observed_at_ms: 2,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 0,
            occurred_at_ms: 1,
            event: AgentProviderSemanticEventV1::ModelOutput {
                text: "hello".into(),
            },
        }],
    };
    page.validate_for(&profile).expect("event page");
    let batch_id = uuid::Uuid::new_v4();
    let accepted = AgentProviderEventReceiptV1::accepted(&profile, batch_id, &page, 3, false)
        .expect("accepted receipt");
    let replay = AgentProviderEventReceiptV1::accepted(&profile, batch_id, &page, 3, true)
        .expect("replay receipt");
    assert_eq!(accepted.page_digest, replay.page_digest);
    assert_eq!(accepted.accepted_after_event_sequence, Some(0));
    assert!(!accepted.replayed);
    assert!(replay.replayed);
}

#[test]
fn event_pages_reject_sequence_gaps_and_mixed_profile_versions() {
    let profile = AgentProviderProfile::parse_acl(REFERENCE_PROFILE).expect("provider profile");
    let other = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("other profile");
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().to_owned(),
        profile.capability_digest().to_owned(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-1".into(),
    )
    .expect("run identity");
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity,
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(1),
        source_last_sequence: Some(1),
        source_event_count: 1,
        latest_sequence_exclusive: 2,
        next_after_event_sequence: Some(1),
        state: AgentProviderRunStateV1::Executing,
        observed_at_ms: 2,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 1,
            occurred_at_ms: 1,
            event: AgentProviderSemanticEventV1::ModelOutput {
                text: "skipped zero".into(),
            },
        }],
    };
    assert!(page.validate_for(&profile).is_err());
    assert!(page.validate_for(&other).is_err());
}

#[test]
fn tool_events_are_capability_bound_and_carry_only_content_identity() {
    let profile = AgentProviderProfile::parse_acl(TOOL_PROFILE).expect("Tool provider profile");
    let tool = HarnessToolBindingV1 {
        name: "workspace.search".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "b".repeat(64)),
        approval_required: false,
    };
    let request = AgentProviderToolPayloadIdentityV1 {
        digest: format!("sha256:{}", "c".repeat(64)),
        size_bytes: 128,
        media_type: "application/json".into(),
    };
    let result = AgentProviderToolPayloadIdentityV1 {
        digest: format!("sha256:{}", "d".repeat(64)),
        size_bytes: 0,
        media_type: "application/json".into(),
    };
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-1".into(),
    )
    .expect("run identity");
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity,
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(0),
        source_last_sequence: Some(1),
        source_event_count: 2,
        latest_sequence_exclusive: 2,
        next_after_event_sequence: Some(1),
        state: AgentProviderRunStateV1::Executing,
        observed_at_ms: 3,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![
            AgentProviderEventRecordV1 {
                sequence: 0,
                occurred_at_ms: 1,
                event: AgentProviderSemanticEventV1::ToolRequest {
                    call_id: "call-1".into(),
                    tool: tool.clone(),
                    request: request.clone(),
                },
            },
            AgentProviderEventRecordV1 {
                sequence: 1,
                occurred_at_ms: 2,
                event: AgentProviderSemanticEventV1::ToolResult {
                    call_id: "call-1".into(),
                    tool,
                    request_digest: request.digest,
                    outcome: AgentProviderToolResultOutcomeV1::Succeeded,
                    result,
                },
            },
        ],
    };
    page.validate_for(&profile).expect("Tool event page");

    let code = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("Code provider profile");
    let mut unsupported = page;
    unsupported.identity = AgentProviderRunIdentityV1::new(
        code.digest().into(),
        code.capability_digest().into(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-1".into(),
    )
    .expect("Code run identity");
    assert!(unsupported.validate_for(&code).is_err());
}

#[test]
fn approval_required_tool_requests_pause_at_one_closed_checkpoint() {
    let profile = AgentProviderProfile::parse_acl(TOOL_PROFILE).expect("Tool provider profile");
    let tool = HarnessToolBindingV1 {
        name: "workspace.publish".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "b".repeat(64)),
        approval_required: true,
    };
    let request = AgentProviderToolPayloadIdentityV1 {
        digest: format!("sha256:{}", "c".repeat(64)),
        size_bytes: 128,
        media_type: "application/json".into(),
    };
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-approval".into(),
    )
    .expect("run identity");
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity,
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(0),
        source_last_sequence: Some(0),
        source_event_count: 1,
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProviderRunStateV1::AwaitingApproval,
        observed_at_ms: 2,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 0,
            occurred_at_ms: 1,
            event: AgentProviderSemanticEventV1::ToolRequest {
                call_id: "call-approval-1".into(),
                tool: tool.clone(),
                request,
            },
        }],
    };
    page.validate_for(&profile)
        .expect("one closed approval checkpoint page");

    let tool_only =
        AgentProviderProfile::parse_acl(TOOL_ONLY_PROFILE).expect("Tool-only provider profile");
    let mut unsupported = page.clone();
    unsupported.identity = AgentProviderRunIdentityV1::new(
        tool_only.digest().into(),
        tool_only.capability_digest().into(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        "execution-approval".into(),
    )
    .expect("Tool-only run identity");
    assert!(unsupported.validate_for(&tool_only).is_err());

    let mut not_paused = page.clone();
    not_paused.state = AgentProviderRunStateV1::Executing;
    assert!(not_paused.validate_for(&profile).is_err());

    let mut hidden_progress = page;
    hidden_progress.source_last_sequence = Some(1);
    hidden_progress.source_event_count = 2;
    hidden_progress.latest_sequence_exclusive = 2;
    hidden_progress.next_after_event_sequence = Some(1);
    assert!(hidden_progress.validate_for(&profile).is_err());
}

#[test]
fn approval_resume_commands_are_identity_bound_and_exactly_replayable() {
    let profile = AgentProviderProfile::parse_acl(TOOL_PROFILE).expect("Tool provider profile");
    let tool = HarnessToolBindingV1 {
        name: "workspace.publish".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "b".repeat(64)),
        approval_required: true,
    };
    let mut invocation = invocation_profile(&profile);
    invocation.tools = vec![tool.clone()];
    invocation.required_capabilities = vec![
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::Cleanup,
        AgentProviderCapabilityV1::EventPages,
        AgentProviderCapabilityV1::PauseResume,
        AgentProviderCapabilityV1::ToolCalls,
    ];
    invocation
        .validate_for(&profile)
        .expect("approval-capable invocation profile");
    let mut missing_pause_resume = invocation.clone();
    missing_pause_resume
        .required_capabilities
        .retain(|capability| *capability != AgentProviderCapabilityV1::PauseResume);
    assert!(missing_pause_resume.validate().is_err());

    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        invocation.agent.artifact_digest.clone(),
        "conversation-1".into(),
        "execution-approval".into(),
    )
    .expect("run identity");
    let decision = AgentProviderApprovalDecisionV1::new(
        "decision-1".into(),
        "checkpoint-1".into(),
        &identity,
        "call-approval-1".into(),
        tool,
        format!("sha256:{}", "c".repeat(64)),
        AgentProviderApprovalOutcomeV1::Approved,
        10,
    )
    .expect("approval decision");
    let command = AgentProviderCommandV1::Resume {
        request: AgentProviderRunResumeV1::new(
            "execution-approval-resume-checkpoint-1".into(),
            identity.clone(),
            decision,
        )
        .expect("resume request"),
    };
    command
        .validate_for(&profile)
        .expect("profile-bound resume command");
    let receipt = AgentProviderCommandReceiptV1::accepted(
        &profile,
        &command,
        AgentProviderRunStateV1::Executing,
        11,
        false,
    )
    .expect("resume receipt");
    let replay = AgentProviderCommandReceiptV1::accepted(
        &profile,
        &command,
        AgentProviderRunStateV1::Executing,
        11,
        true,
    )
    .expect("resume replay receipt");
    assert_eq!(receipt.command_digest, replay.command_digest);
    assert!(!receipt.replayed);
    assert!(replay.replayed);

    let mut changed = command.clone();
    let AgentProviderCommandV1::Resume { request } = &mut changed else {
        unreachable!("resume command")
    };
    request.decision.request_digest = format!("sha256:{}", "d".repeat(64));
    assert!(receipt.validate_for(&profile, &changed).is_err());

    let other_identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        invocation.agent.artifact_digest,
        "conversation-1".into(),
        "execution-other".into(),
    )
    .expect("other run identity");
    let AgentProviderCommandV1::Resume { request } = command else {
        unreachable!("resume command")
    };
    assert!(AgentProviderRunResumeV1 {
        schema: AgentProviderRunResumeV1::SCHEMA.into(),
        request_id: "mismatched-resume".into(),
        identity: other_identity,
        decision: request.decision,
    }
    .validate()
    .is_err());
}
