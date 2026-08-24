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
        local_transform: None,
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
fn variable_aggregate_configuration_is_closed_ordered_and_versioned() {
    let fixture = include_str!("../../../../../../../contracts/w0.3/variable-aggregate.acl");
    WorkflowPayload::parse_acl(WorkflowPayloadKind::Configuration, fixture)
        .expect("Variable Aggregator conformance fixture");

    let configuration = WorkflowVariableAggregateConfiguration {
        group_enabled: true,
        groups: vec![
            WorkflowVariableAggregateGroup {
                output_port: "primary".into(),
                output_type: WorkflowDataType::String,
                candidates: vec![
                    WorkflowVariableAggregateCandidate {
                        input_port: "left".into(),
                        ordinal: 0,
                    },
                    WorkflowVariableAggregateCandidate {
                        input_port: "right".into(),
                        ordinal: 1,
                    },
                ],
            },
            WorkflowVariableAggregateGroup {
                output_port: "secondary".into(),
                output_type: WorkflowDataType::Number,
                candidates: vec![WorkflowVariableAggregateCandidate {
                    input_port: "fallback".into(),
                    ordinal: 0,
                }],
            },
        ],
    };
    let mut step = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    step.local_transform = Some(WorkflowLocalTransformConfiguration::VariableAggregate(
        configuration.clone(),
    ));
    let payload = WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(step))
        .expect("Variable Aggregator configuration");

    let mut reordered = configuration.clone();
    reordered.groups.reverse();
    for group in &mut reordered.groups {
        group.candidates.reverse();
    }
    let mut reordered_step = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    reordered_step.local_transform = Some(WorkflowLocalTransformConfiguration::VariableAggregate(
        reordered,
    ));
    let reordered_payload =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(reordered_step))
            .expect("reordered Variable Aggregator configuration");

    assert_eq!(
        payload.schema(),
        WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA
    );
    assert_eq!(payload.canonical_acl(), reordered_payload.canonical_acl());
    assert_eq!(payload.digest(), reordered_payload.digest());
    assert!(payload.canonical_acl().contains("group_enabled = true"));
    assert!(payload.canonical_acl().contains("candidate \"left\""));
    assert_eq!(
        WorkflowPayload::restore(
            WorkflowPayloadKind::Configuration,
            payload.canonical_acl(),
            payload.digest().as_str(),
        )
        .expect("restored Variable Aggregator")
        .content(),
        &WorkflowPayloadContent::Configuration(WorkflowStepConfiguration {
            step_kind: WorkflowStepKind::Transform,
            template: None,
            selector: None,
            default_handle: None,
            message: None,
            details: None,
            expires_after_seconds: None,
            routes: Vec::new(),
            local_transform: Some(WorkflowLocalTransformConfiguration::VariableAggregate(
                configuration,
            )),
        })
    );

    let v1_claim = payload.canonical_acl().replace(
        WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA,
        WORKFLOW_CONFIGURATION_SCHEMA,
    );
    assert!(WorkflowPayload::parse_acl(WorkflowPayloadKind::Configuration, &v1_claim).is_err());
}

#[test]
fn variable_aggregate_configuration_rejects_ambiguous_priority_and_simple_groups() {
    let candidate = |input_port: &str, ordinal| WorkflowVariableAggregateCandidate {
        input_port: input_port.into(),
        ordinal,
    };
    let mut configuration = WorkflowVariableAggregateConfiguration {
        group_enabled: false,
        groups: vec![WorkflowVariableAggregateGroup {
            output_port: "result".into(),
            output_type: WorkflowDataType::String,
            candidates: vec![candidate("left", 0)],
        }],
    };
    assert!(configuration.validate().is_err());

    configuration.groups[0].output_port = "output".into();
    configuration.groups[0].candidates = vec![candidate("left", 0), candidate("right", 0)];
    assert!(configuration.validate().is_err());

    configuration.groups[0].candidates = vec![candidate("left", 0), candidate("right", 1)];
    configuration.validate().expect("valid simple aggregation");
}

#[test]
fn list_operator_configuration_is_closed_ordered_and_versioned() {
    let fixture = include_str!("../../../../../../../contracts/w0.3/list-operator.acl");
    let parsed = WorkflowPayload::parse_acl(WorkflowPayloadKind::Configuration, fixture)
        .expect("List Operator conformance fixture");
    assert_eq!(parsed.schema(), WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA);

    let configuration = WorkflowListOperatorConfiguration {
        source_port: "items".into(),
        item_type: WorkflowDataType::Object,
        conditions: vec![
            WorkflowListOperatorFilterCondition {
                id: "minimum_size".into(),
                ordinal: 1,
                key: Some("size".into()),
                value_type: WorkflowDataType::Number,
                operator: WorkflowListOperatorFilterOperator::GreaterThanOrEqual,
                operand: Some(WorkflowListOperatorOperand::InputPort {
                    input_port: "minimum_size".into(),
                    value_type: WorkflowDataType::Number,
                }),
            },
            WorkflowListOperatorFilterCondition {
                id: "supported_type".into(),
                ordinal: 0,
                key: Some("type".into()),
                value_type: WorkflowDataType::String,
                operator: WorkflowListOperatorFilterOperator::In,
                operand: Some(WorkflowListOperatorOperand::Literal(serde_json::json!([
                    "document", "image"
                ]))),
            },
        ],
        extract: Some(WorkflowListOperatorExtract::InputPort {
            input_port: "serial".into(),
        }),
        order: Some(WorkflowListOperatorOrder {
            key: Some("size".into()),
            value_type: WorkflowDataType::Number,
            direction: WorkflowListOperatorOrderDirection::Desc,
        }),
        limit: Some(5),
    };
    let mut step = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    step.local_transform = Some(WorkflowLocalTransformConfiguration::ListOperator(
        configuration.clone(),
    ));
    let payload = WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(step))
        .expect("List Operator configuration");

    assert_eq!(payload.canonical_acl(), parsed.canonical_acl());
    assert_eq!(payload.digest(), parsed.digest());
    assert_eq!(
        WorkflowPayload::restore(
            WorkflowPayloadKind::Configuration,
            payload.canonical_acl(),
            payload.digest().as_str(),
        )
        .expect("restored List Operator")
        .content(),
        &WorkflowPayloadContent::Configuration(WorkflowStepConfiguration {
            step_kind: WorkflowStepKind::Transform,
            template: None,
            selector: None,
            default_handle: None,
            message: None,
            details: None,
            expires_after_seconds: None,
            routes: Vec::new(),
            local_transform: Some(WorkflowLocalTransformConfiguration::ListOperator(
                configuration,
            )),
        })
    );

    let legacy_claim = payload.canonical_acl().replace(
        WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA,
        WORKFLOW_CONFIGURATION_SCHEMA,
    );
    assert!(WorkflowPayload::parse_acl(WorkflowPayloadKind::Configuration, &legacy_claim).is_err());
}

#[test]
fn list_operator_configuration_rejects_ambiguous_or_unbounded_operations() {
    let condition = || WorkflowListOperatorFilterCondition {
        id: "minimum".into(),
        ordinal: 0,
        key: None,
        value_type: WorkflowDataType::Number,
        operator: WorkflowListOperatorFilterOperator::GreaterThan,
        operand: Some(WorkflowListOperatorOperand::Literal(serde_json::json!(3))),
    };
    let mut configuration = WorkflowListOperatorConfiguration {
        source_port: "items".into(),
        item_type: WorkflowDataType::Number,
        conditions: vec![condition()],
        extract: None,
        order: None,
        limit: Some(10),
    };
    configuration
        .validate()
        .expect("valid numeric List Operator");

    configuration.conditions.push(condition());
    assert!(configuration.validate().is_err());
    configuration.conditions.pop();

    configuration.conditions[0].operator = WorkflowListOperatorFilterOperator::Contains;
    assert!(configuration.validate().is_err());
    configuration.conditions[0] = condition();

    configuration.item_type = WorkflowDataType::String;
    configuration.conditions[0] = WorkflowListOperatorFilterCondition {
        id: "member".into(),
        ordinal: 0,
        key: None,
        value_type: WorkflowDataType::String,
        operator: WorkflowListOperatorFilterOperator::In,
        operand: Some(WorkflowListOperatorOperand::Literal(serde_json::json!([
            "alpha", "beta"
        ]))),
    };
    assert!(configuration.validate().is_err());
    configuration.item_type = WorkflowDataType::Number;
    configuration.conditions[0] = condition();

    configuration.extract = Some(WorkflowListOperatorExtract::Literal { index: 0 });
    assert!(configuration.validate().is_err());
    configuration.extract = None;

    configuration.limit = Some(WORKFLOW_LIST_OPERATOR_MAX_ITEMS + 1);
    assert!(configuration.validate().is_err());
}

#[test]
fn list_operator_object_operations_use_the_closed_file_field_matrix() {
    let mut configuration = WorkflowListOperatorConfiguration {
        source_port: "items".into(),
        item_type: WorkflowDataType::Object,
        conditions: vec![WorkflowListOperatorFilterCondition {
            id: "supported_type".into(),
            ordinal: 0,
            key: Some("type".into()),
            value_type: WorkflowDataType::String,
            operator: WorkflowListOperatorFilterOperator::In,
            operand: Some(WorkflowListOperatorOperand::Literal(serde_json::json!([
                "document", "image"
            ]))),
        }],
        extract: None,
        order: Some(WorkflowListOperatorOrder {
            key: Some("size".into()),
            value_type: WorkflowDataType::Number,
            direction: WorkflowListOperatorOrderDirection::Asc,
        }),
        limit: None,
    };
    configuration
        .validate()
        .expect("valid file-compatible object operations");

    configuration.conditions[0].operator = WorkflowListOperatorFilterOperator::Equals;
    assert!(configuration.validate().is_err());
    configuration.conditions[0].operator = WorkflowListOperatorFilterOperator::In;

    configuration.conditions[0].key = Some("owner".into());
    assert!(configuration.validate().is_err());
    configuration.conditions[0].key = Some("type".into());

    configuration.order.as_mut().expect("order").value_type = WorkflowDataType::String;
    assert!(configuration.validate().is_err());
    configuration.order.as_mut().expect("order").value_type = WorkflowDataType::Number;
    configuration.order.as_mut().expect("order").key = Some("owner".into());
    assert!(configuration.validate().is_err());
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
