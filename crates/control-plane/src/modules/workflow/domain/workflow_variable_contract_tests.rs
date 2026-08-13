use super::*;
use crate::modules::shared_kernel::domain::Sha256Digest;

mod graph_tests;

const VARIABLE_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.3/variable-contract.acl"
));

fn digest(byte: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
}

fn declaration(
    name: &str,
    scope: WorkflowVariableScope,
    value_type: WorkflowDataType,
    schema: char,
) -> WorkflowVariableDeclaration {
    WorkflowVariableDeclaration {
        name: name.into(),
        scope,
        value_type,
        value_schema_digest: digest(schema),
        source_schema_digest: None,
        storage_class: WorkflowVariableStorageClass::Inline,
        mutation_mode: WorkflowVariableMutationMode::Immutable,
        required: true,
        source_step_id: None,
        source_path: Vec::new(),
        region_id: None,
        default_value_digest: None,
    }
}

fn contract_spec() -> WorkflowVariableContractSpec {
    let mut request = declaration(
        "request",
        WorkflowVariableScope::InvocationInput,
        WorkflowDataType::Object,
        'a',
    );
    request.source_schema_digest = Some(digest('a'));
    let mut triage_result = declaration(
        "triage_result",
        WorkflowVariableScope::NodeOutput,
        WorkflowDataType::Object,
        'b',
    );
    triage_result.source_step_id = Some("triage".into());
    triage_result.source_schema_digest = Some(digest('b'));

    let mut summary = declaration(
        "summary",
        WorkflowVariableScope::Run,
        WorkflowDataType::String,
        'c',
    );
    summary.mutation_mode = WorkflowVariableMutationMode::Deterministic;

    let mut api_secret = declaration(
        "api_secret",
        WorkflowVariableScope::InvocationInput,
        WorkflowDataType::Object,
        'd',
    );
    api_secret.storage_class = WorkflowVariableStorageClass::SecretReference;
    api_secret.source_path = vec!["api_secret".into()];
    api_secret.source_schema_digest = Some(digest('a'));
    api_secret.required = false;

    let mut attachment = declaration(
        "attachment",
        WorkflowVariableScope::InvocationInput,
        WorkflowDataType::Object,
        'e',
    );
    attachment.storage_class = WorkflowVariableStorageClass::ImmutableObjectReference;
    attachment.source_path = vec!["attachment".into()];
    attachment.source_schema_digest = Some(digest('a'));
    attachment.required = false;

    WorkflowVariableContractSpec {
        id: "support.triage".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![summary, request, attachment, triage_result, api_secret],
        reads: vec![
            WorkflowVariableRead {
                id: "triage-request".into(),
                variable: "request".into(),
                consumer_step_id: "triage".into(),
                consumer_region_id: None,
                target_port: "request".into(),
                path: Vec::new(),
                expected_type: WorkflowDataType::Object,
                expected_schema_digest: digest('a'),
                required: true,
                mode: WorkflowVariableReadMode::DirectValue,
            },
            WorkflowVariableRead {
                id: "triage-secret".into(),
                variable: "api_secret".into(),
                consumer_step_id: "triage".into(),
                consumer_region_id: None,
                target_port: "secret".into(),
                path: Vec::new(),
                expected_type: WorkflowDataType::Object,
                expected_schema_digest: digest('d'),
                required: false,
                mode: WorkflowVariableReadMode::OpaqueReference,
            },
            WorkflowVariableRead {
                id: "triage-attachment".into(),
                variable: "attachment".into(),
                consumer_step_id: "triage".into(),
                consumer_region_id: None,
                target_port: "attachment".into(),
                path: Vec::new(),
                expected_type: WorkflowDataType::Object,
                expected_schema_digest: digest('e'),
                required: false,
                mode: WorkflowVariableReadMode::OpaqueReference,
            },
            WorkflowVariableRead {
                id: "output-summary".into(),
                variable: "summary".into(),
                consumer_step_id: "output".into(),
                consumer_region_id: None,
                target_port: "result".into(),
                path: Vec::new(),
                expected_type: WorkflowDataType::String,
                expected_schema_digest: digest('c'),
                required: true,
                mode: WorkflowVariableReadMode::DirectValue,
            },
        ],
        assignments: vec![WorkflowVariableAssignment {
            id: "assign-summary".into(),
            target_variable: "summary".into(),
            source_variable: "triage_result".into(),
            writer_step_id: "triage".into(),
            writer_region_id: None,
            source_path: vec!["summary".into()],
            value_type: WorkflowDataType::String,
            value_schema_digest: digest('c'),
            mutation_order: 1,
            expected_revision_variable: None,
            idempotency_key_variable: None,
        }],
        exports: Vec::new(),
    }
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec {
        name: "Support triage".into(),
        description: String::new(),
        steps: vec![
            step("output", WorkflowStepKind::Output, 'c', 'c'),
            step("triage", WorkflowStepKind::Transform, 'a', 'b'),
            step("input", WorkflowStepKind::Input, 'a', 'a'),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-triage".into(),
                source: "input".into(),
                target: "triage".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "triage-output".into(),
                source: "triage".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    }
}

fn step(id: &str, kind: WorkflowStepKind, input: char, output: char) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest: digest('f'),
        input_schema_digest: digest(input),
        output_schema_digest: digest(output),
        policy_digest: None,
        capability: None,
    }
}

#[test]
fn variable_contract_is_canonical_digest_addressed_and_restorable() {
    let contract = WorkflowVariableContract::from_spec(contract_spec()).expect("contract");
    assert_eq!(contract.id(), "support.triage");
    assert_eq!(contract.revision(), "1.0.0");
    assert_eq!(contract.compiler_schema_version(), 2);
    assert!(contract.digest().as_str().starts_with("sha256:"));
    assert!(contract.canonical_acl().ends_with('\n'));
    assert_eq!(
        WorkflowVariableContract::restore(contract.canonical_acl(), contract.digest().as_str())
            .expect("restored"),
        contract
    );
    assert_eq!(
        WorkflowVariableContract::parse_acl(&contract.canonical_acl().replace('\n', "\r\n"))
            .expect("CRLF"),
        contract
    );
    let bare_cr = contract.canonical_acl().replacen('\n', "\r", 1);
    assert!(WorkflowVariableContract::parse_acl(&bare_cr).is_err());
}

#[test]
fn checked_in_variable_fixture_matches_the_domain_generator() {
    let generated = WorkflowVariableContract::from_spec(contract_spec()).expect("contract");
    assert_eq!(
        VARIABLE_FIXTURE.replace("\r\n", "\n"),
        generated.canonical_acl()
    );
    assert_eq!(
        WorkflowVariableContract::parse_acl(VARIABLE_FIXTURE).expect("fixture"),
        generated
    );
}

#[test]
fn variable_acl_rejects_shape_schema_and_revision_drift() {
    let fixture = VARIABLE_FIXTURE.replace("\r\n", "\n");
    assert!(WorkflowVariableContract::parse_acl(&fixture.replacen(
        "  compiler_schema_version = 2\n",
        "  compiler_schema_version = 2\n  unknown = true\n",
        1,
    ))
    .is_err());
    assert!(WorkflowVariableContract::parse_acl(&fixture.replacen(
        WORKFLOW_VARIABLE_CONTRACT_SCHEMA,
        "cloud.workflow.variable-contract.v2",
        1,
    ))
    .is_err());
    assert!(WorkflowVariableContract::parse_acl(&fixture.replacen(
        "  revision = \"1.0.0\"\n",
        "  revision = \"latest\"\n",
        1,
    ))
    .is_err());
    assert!(WorkflowVariableContract::parse_acl(&fixture.replacen(
        "  compiler_schema_version = 2\n",
        "  compiler_schema_version = 3\n",
        1,
    ))
    .is_err());
}

#[test]
fn declaration_scope_storage_and_mutation_boundaries_fail_closed() {
    let mut spec = contract_spec();
    let secret = spec
        .declarations
        .iter_mut()
        .find(|value| value.name == "api_secret")
        .expect("secret");
    secret.value_type = WorkflowDataType::String;
    assert!(WorkflowVariableContract::from_spec(spec).is_err());

    let mut spec = contract_spec();
    let output = spec
        .declarations
        .iter_mut()
        .find(|value| value.name == "triage_result")
        .expect("output");
    output.mutation_mode = WorkflowVariableMutationMode::Deterministic;
    assert!(WorkflowVariableContract::from_spec(spec).is_err());

    let mut spec = contract_spec();
    let run = spec
        .declarations
        .iter_mut()
        .find(|value| value.name == "summary")
        .expect("run");
    run.source_step_id = Some("triage".into());
    assert!(WorkflowVariableContract::from_spec(spec).is_err());
}

#[test]
fn reads_enforce_direct_opaque_and_application_port_modes() {
    let mut spec = contract_spec();
    spec.reads
        .iter_mut()
        .find(|value| value.variable == "api_secret")
        .expect("secret read")
        .mode = WorkflowVariableReadMode::DirectValue;
    assert!(WorkflowVariableContract::from_spec(spec).is_err());

    let mut spec = contract_spec();
    let request = spec
        .reads
        .iter_mut()
        .find(|value| value.variable == "request")
        .expect("request read");
    request.path.push("ticket".into());
    request.expected_type = WorkflowDataType::String;
    request.expected_schema_digest = digest('9');
    assert!(WorkflowVariableContract::from_spec(spec).is_ok());

    let mut spec = contract_spec();
    let request_declaration = spec
        .declarations
        .iter_mut()
        .find(|value| value.name == "request")
        .expect("request");
    request_declaration.value_type = WorkflowDataType::String;
    assert!(WorkflowVariableContract::from_spec(spec).is_err());
}

#[test]
fn assignments_require_unique_order_and_optimistic_application_evidence() {
    let mut duplicate = contract_spec();
    let mut second = duplicate.assignments[0].clone();
    second.id = "assign-summary-again".into();
    duplicate.assignments.push(second);
    assert!(WorkflowVariableContract::from_spec(duplicate).is_err());

    let mut application = contract_spec();
    let mut conversation = declaration(
        "conversation_topic",
        WorkflowVariableScope::Application,
        WorkflowDataType::String,
        '7',
    );
    conversation.mutation_mode = WorkflowVariableMutationMode::OptimisticApplicationPort;
    application.declarations.push(conversation);
    application.assignments.push(WorkflowVariableAssignment {
        id: "assign-conversation-topic".into(),
        target_variable: "conversation_topic".into(),
        source_variable: "summary".into(),
        writer_step_id: "output".into(),
        writer_region_id: None,
        source_path: Vec::new(),
        value_type: WorkflowDataType::String,
        value_schema_digest: digest('7'),
        mutation_order: 1,
        expected_revision_variable: None,
        idempotency_key_variable: None,
    });
    assert!(WorkflowVariableContract::from_spec(application).is_err());
}

#[test]
fn application_assignment_freezes_evidence_but_waits_for_descriptor_bound_plan() {
    let mut spec = contract_spec();
    let mut conversation = declaration(
        "conversation_topic",
        WorkflowVariableScope::Application,
        WorkflowDataType::String,
        'c',
    );
    conversation.mutation_mode = WorkflowVariableMutationMode::OptimisticApplicationPort;
    let mut revision = declaration(
        "conversation_revision",
        WorkflowVariableScope::InvocationInput,
        WorkflowDataType::Number,
        '4',
    );
    revision.source_path = vec!["conversation_revision".into()];
    revision.source_schema_digest = Some(digest('a'));
    let mut idempotency_key = declaration(
        "idempotency_key",
        WorkflowVariableScope::InvocationInput,
        WorkflowDataType::String,
        '3',
    );
    idempotency_key.source_path = vec!["idempotency_key".into()];
    idempotency_key.source_schema_digest = Some(digest('a'));
    spec.declarations
        .extend([conversation, revision, idempotency_key]);
    spec.assignments.push(WorkflowVariableAssignment {
        id: "assign-conversation-topic".into(),
        target_variable: "conversation_topic".into(),
        source_variable: "summary".into(),
        writer_step_id: "output".into(),
        writer_region_id: None,
        source_path: Vec::new(),
        value_type: WorkflowDataType::String,
        value_schema_digest: digest('c'),
        mutation_order: 1,
        expected_revision_variable: Some("conversation_revision".into()),
        idempotency_key_variable: Some("idempotency_key".into()),
    });
    let contract = WorkflowVariableContract::from_spec(spec).expect("application contract");
    assert!(contract.validate_graph_bindings(&workflow_spec()).is_err());
}

#[test]
fn pinned_default_allows_a_required_read_before_any_assignment() {
    let mut spec = contract_spec();
    spec.assignments.clear();
    let summary = spec
        .declarations
        .iter_mut()
        .find(|value| value.name == "summary")
        .expect("summary");
    summary.required = false;
    summary.default_value_digest = Some(digest('0'));
    let contract = WorkflowVariableContract::from_spec(spec).expect("default contract");
    contract
        .validate_graph_bindings(&workflow_spec())
        .expect("default-covered read");
}

#[test]
fn composite_locals_leave_only_through_exact_exports() {
    let mut spec = contract_spec();
    let mut local = declaration(
        "item_result",
        WorkflowVariableScope::CompositeLocal,
        WorkflowDataType::String,
        '6',
    );
    local.region_id = Some("iteration".into());
    local.mutation_mode = WorkflowVariableMutationMode::Deterministic;
    spec.declarations.push(local);
    let mut region_output = declaration(
        "iteration_result",
        WorkflowVariableScope::NodeOutput,
        WorkflowDataType::String,
        '6',
    );
    region_output.source_step_id = Some("iteration".into());
    region_output.source_schema_digest = Some(digest('6'));
    spec.declarations.push(region_output);
    spec.exports.push(WorkflowVariableExport {
        id: "export-item-result".into(),
        region_id: "iteration".into(),
        source_variable: "item_result".into(),
        target_variable: "iteration_result".into(),
        source_path: Vec::new(),
        value_type: WorkflowDataType::String,
        value_schema_digest: digest('6'),
    });
    assert!(WorkflowVariableContract::from_spec(spec.clone()).is_ok());
    spec.exports[0].region_id = "other".into();
    assert!(WorkflowVariableContract::from_spec(spec).is_err());

    let mut assignment_escape = contract_spec();
    assignment_escape.assignments.clear();
    let mut local = declaration(
        "item_result",
        WorkflowVariableScope::CompositeLocal,
        WorkflowDataType::String,
        'c',
    );
    local.region_id = Some("iteration".into());
    local.mutation_mode = WorkflowVariableMutationMode::Deterministic;
    assignment_escape.declarations.push(local);
    assignment_escape
        .assignments
        .push(WorkflowVariableAssignment {
            id: "escape-item-result".into(),
            target_variable: "summary".into(),
            source_variable: "item_result".into(),
            writer_step_id: "iteration".into(),
            writer_region_id: Some("iteration".into()),
            source_path: Vec::new(),
            value_type: WorkflowDataType::String,
            value_schema_digest: digest('c'),
            mutation_order: 1,
            expected_revision_variable: None,
            idempotency_key_variable: None,
        });
    assert!(WorkflowVariableContract::from_spec(assignment_escape).is_err());
}

#[test]
fn composite_region_inputs_are_immutable_and_locals_are_deterministic() {
    let mut spec = contract_spec();
    let mut item = declaration(
        "item",
        WorkflowVariableScope::CompositeLocal,
        WorkflowDataType::Object,
        '5',
    );
    item.region_id = Some("iteration".into());
    item.source_step_id = Some("iteration".into());
    item.source_path = vec!["item".into()];
    item.source_schema_digest = Some(digest('5'));
    let mut immutable = spec.clone();
    immutable.declarations.push(item.clone());
    assert!(WorkflowVariableContract::from_spec(immutable).is_ok());

    item.mutation_mode = WorkflowVariableMutationMode::Deterministic;
    spec.declarations.push(item);
    assert!(WorkflowVariableContract::from_spec(spec).is_err());
}

#[test]
fn variable_contracts_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowVariableContract>();
    assert_send_sync::<WorkflowVariableContractSpec>();
}
