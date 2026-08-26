use a3s_cloud_contracts::{
    AgentProviderCapabilityRequirementsV1, AgentProviderCapabilityV1,
    AgentProviderCommandReceiptV1, AgentProviderCommandV1, AgentProviderEventPageRequestV1,
    AgentProviderEventPageV1, AgentProviderEventReceiptV1, AgentProviderEventRecordV1,
    AgentProviderProfile, AgentProviderRunIdentityV1, AgentProviderRunStartV1,
    AgentProviderRunStateV1, AgentProviderSemanticEventV1, AGENT_PROVIDER_MAX_EVENTS_PER_PAGE,
    AGENT_PROVIDER_PROTOCOL_V1,
};

const CODE_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/a3s-code-provider-profile.acl"
));
const REFERENCE_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));

#[test]
fn immutable_profiles_bind_canonical_acl_and_capabilities() {
    let code = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("Code provider profile");
    let reference =
        AgentProviderProfile::parse_acl(REFERENCE_PROFILE).expect("reference provider profile");

    assert_eq!(code.kind(), "a3s.code");
    assert_eq!(code.revision(), "8.0.1");
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
