use super::*;

#[test]
fn payloads_are_closed_canonical_and_digest_verified() {
    let acl = r#"
configuration {
  step_kind = "transform"
  template = "Hello {{input.name}}"
  schema = "cloud.workflow.configuration.v1"
}
"#;
    let payload =
        WorkflowPayload::parse_acl(WorkflowPayloadKind::Configuration, acl).expect("payload");
    assert_eq!(payload.kind(), WorkflowPayloadKind::Configuration);
    assert!(payload.digest().as_str().starts_with("sha256:"));
    assert_eq!(
        WorkflowPayload::restore(
            WorkflowPayloadKind::Configuration,
            payload.canonical_acl(),
            payload.digest().as_str(),
        )
        .expect("restore"),
        payload
    );
    assert!(WorkflowPayload::restore(
        WorkflowPayloadKind::Configuration,
        payload.canonical_acl(),
        &format!("sha256:{}", "0".repeat(64)),
    )
    .is_err());
    assert!(WorkflowPayload::parse_acl(
        WorkflowPayloadKind::Configuration,
        &acl.replace("template =", "unknown = \"x\"\n  template ="),
    )
    .is_err());
}

#[test]
fn branch_candidates_and_human_expiry_fail_closed() {
    let branch = WorkflowStepConfiguration {
        step_kind: WorkflowStepKind::Branch,
        template: None,
        selector: Some("input.kind".into()),
        default_handle: Some("other".into()),
        message: None,
        details: None,
        expires_after_seconds: None,
        routes: vec![
            WorkflowBranchRoute {
                handle: "fix".into(),
                equals: "fix".into(),
            },
            WorkflowBranchRoute {
                handle: "other".into(),
                equals: "other".into(),
            },
        ],
    };
    WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(branch.clone()))
        .expect("branch");
    let mut duplicate = branch;
    duplicate.routes[1].handle = "fix".into();
    assert!(duplicate.validate().is_err());

    let mut decision = WorkflowStepConfiguration::empty(WorkflowStepKind::HumanDecision);
    decision.message = Some("Approve?".into());
    decision.expires_after_seconds = Some(30 * 24 * 60 * 60 + 1);
    assert!(decision.validate().is_err());
}

#[test]
fn policy_records_dynamic_choice_inputs() {
    let policy = WorkflowPolicy {
        mode: WorkflowPolicyMode::RecordedChoice,
        expression: Some("input.priority".into()),
        candidates: vec![
            WorkflowPolicyCandidate {
                id: "standard".into(),
                digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            },
            WorkflowPolicyCandidate {
                id: "urgent".into(),
                digest: Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("digest"),
            },
        ],
        retry: None,
        default_output: None,
    };
    let payload =
        WorkflowPayload::from_content(WorkflowPayloadContent::Policy(policy)).expect("policy");
    assert_eq!(payload.schema(), WORKFLOW_POLICY_SCHEMA);
}

#[test]
fn policy_v2_freezes_a_bounded_provider_retry_budget_without_changing_v1() {
    let legacy = WorkflowPolicy {
        mode: WorkflowPolicyMode::Static,
        expression: None,
        candidates: Vec::new(),
        retry: None,
        default_output: None,
    };
    let legacy_payload =
        WorkflowPayload::from_content(WorkflowPayloadContent::Policy(legacy.clone()))
            .expect("legacy policy");
    assert_eq!(legacy_payload.schema(), WORKFLOW_POLICY_SCHEMA);
    assert!(!legacy_payload.canonical_acl().contains("retry"));
    assert!(!serde_json::to_value(legacy)
        .expect("legacy policy JSON")
        .as_object()
        .expect("legacy policy object")
        .contains_key("retry"));

    let policy = WorkflowPolicy {
        mode: WorkflowPolicyMode::Static,
        expression: None,
        candidates: Vec::new(),
        retry: Some(WorkflowRetryPolicy {
            maximum_attempts: 4,
            default_delay_seconds: 15,
        }),
        default_output: None,
    };
    let payload = WorkflowPayload::from_content(WorkflowPayloadContent::Policy(policy.clone()))
        .expect("retry policy");
    assert_eq!(payload.schema(), WORKFLOW_POLICY_SCHEMA_V2);
    assert!(payload
        .canonical_acl()
        .contains("schema = \"cloud.workflow.policy.v2\""));
    assert!(payload.canonical_acl().contains("maximum_attempts = 4"));
    assert!(payload
        .canonical_acl()
        .contains("default_delay_seconds = 15"));
    assert_eq!(
        WorkflowPayload::restore(
            WorkflowPayloadKind::Policy,
            payload.canonical_acl(),
            payload.digest().as_str(),
        )
        .expect("restore retry policy")
        .content(),
        &WorkflowPayloadContent::Policy(policy)
    );

    let v1_with_retry = payload
        .canonical_acl()
        .replace(WORKFLOW_POLICY_SCHEMA_V2, WORKFLOW_POLICY_SCHEMA);
    assert!(WorkflowPayload::parse_acl(WorkflowPayloadKind::Policy, &v1_with_retry).is_err());
    let v2_without_retry = legacy_payload
        .canonical_acl()
        .replace(WORKFLOW_POLICY_SCHEMA, WORKFLOW_POLICY_SCHEMA_V2);
    assert!(WorkflowPayload::parse_acl(WorkflowPayloadKind::Policy, &v2_without_retry).is_err());
}

#[test]
fn provider_retry_budget_rejects_unbounded_or_choice_semantics() {
    for retry in [
        WorkflowRetryPolicy {
            maximum_attempts: 0,
            default_delay_seconds: 1,
        },
        WorkflowRetryPolicy {
            maximum_attempts: WORKFLOW_RETRY_MAXIMUM_ATTEMPTS + 1,
            default_delay_seconds: 1,
        },
        WorkflowRetryPolicy {
            maximum_attempts: 1,
            default_delay_seconds: 0,
        },
        WorkflowRetryPolicy {
            maximum_attempts: 1,
            default_delay_seconds: WORKFLOW_RETRY_MAXIMUM_DEFAULT_DELAY_SECONDS + 1,
        },
    ] {
        assert!(retry.validate().is_err());
    }

    let choice_with_retry = WorkflowPolicy {
        mode: WorkflowPolicyMode::RecordedChoice,
        expression: Some("input.priority".into()),
        candidates: vec![
            WorkflowPolicyCandidate {
                id: "normal".into(),
                digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            },
            WorkflowPolicyCandidate {
                id: "urgent".into(),
                digest: Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("digest"),
            },
        ],
        retry: Some(WorkflowRetryPolicy {
            maximum_attempts: 2,
            default_delay_seconds: 1,
        }),
        default_output: None,
    };
    assert!(choice_with_retry.validate().is_err());
}

#[test]
fn policy_v3_freezes_canonical_default_output_without_changing_older_policies() {
    let output = WorkflowDefaultOutput::new(
        "result",
        serde_json::json!({"message": "unavailable", "retryable": false}),
    )
    .expect("default output");
    let policy = WorkflowPolicy {
        mode: WorkflowPolicyMode::Static,
        expression: None,
        candidates: Vec::new(),
        retry: None,
        default_output: Some(output.clone()),
    };
    let mixed_ownership = WorkflowPolicy {
        retry: Some(WorkflowRetryPolicy {
            maximum_attempts: 2,
            default_delay_seconds: 1,
        }),
        ..policy.clone()
    };
    assert!(mixed_ownership.validate().is_err());
    let payload = WorkflowPayload::from_content(WorkflowPayloadContent::Policy(policy.clone()))
        .expect("default-output policy");
    assert_eq!(payload.schema(), WORKFLOW_POLICY_SCHEMA_V3);
    assert!(payload
        .canonical_acl()
        .contains("schema = \"cloud.workflow.policy.v3\""));
    assert!(payload
        .canonical_acl()
        .contains("default_output \"result\""));
    assert_eq!(
        WorkflowPayload::restore(
            WorkflowPayloadKind::Policy,
            payload.canonical_acl(),
            payload.digest().as_str(),
        )
        .expect("restore default-output policy")
        .content(),
        &WorkflowPayloadContent::Policy(policy)
    );

    let drifted = payload.canonical_acl().replace(
        output.digest.as_str(),
        &format!("sha256:{}", "0".repeat(64)),
    );
    assert!(WorkflowPayload::parse_acl(WorkflowPayloadKind::Policy, &drifted).is_err());
    let v2_claim = payload
        .canonical_acl()
        .replace(WORKFLOW_POLICY_SCHEMA_V3, WORKFLOW_POLICY_SCHEMA_V2);
    assert!(WorkflowPayload::parse_acl(WorkflowPayloadKind::Policy, &v2_claim).is_err());
}
