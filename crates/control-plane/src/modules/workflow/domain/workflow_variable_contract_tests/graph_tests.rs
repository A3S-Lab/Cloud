use super::*;

#[test]
fn graph_bindings_enforce_schema_ancestry_and_required_dominance() {
    let contract = WorkflowVariableContract::from_spec(contract_spec()).expect("contract");
    contract
        .validate_graph_bindings(&workflow_spec())
        .expect("graph bindings");

    let mut schema_drift = workflow_spec();
    schema_drift
        .steps
        .iter_mut()
        .find(|step| step.id == "triage")
        .expect("triage")
        .output_schema_digest = digest('9');
    assert!(contract.validate_graph_bindings(&schema_drift).is_err());

    let mut spec = contract_spec();
    spec.reads
        .iter_mut()
        .find(|read| read.id == "output-summary")
        .expect("summary read")
        .consumer_step_id = "input".into();
    let contract = WorkflowVariableContract::from_spec(spec).expect("contract");
    assert!(contract.validate_graph_bindings(&workflow_spec()).is_err());
}

#[test]
fn reads_bind_the_latest_preceding_assignment_not_a_future_write() {
    let mut spec = contract_spec();
    spec.reads
        .iter_mut()
        .find(|read| read.id == "output-summary")
        .expect("summary read")
        .consumer_step_id = "inspect".into();
    let mut later = spec.assignments[0].clone();
    later.id = "assign-summary-later".into();
    later.writer_step_id = "output".into();
    later.mutation_order = 2;
    spec.assignments.push(later);
    let contract = WorkflowVariableContract::from_spec(spec).expect("contract");

    let mut workflow = workflow_spec();
    workflow
        .steps
        .push(step("inspect", WorkflowStepKind::Transform, 'c', 'c'));
    workflow.edges = vec![
        WorkflowEdgeSpec {
            id: "input-triage".into(),
            source: "input".into(),
            target: "triage".into(),
            source_handle: None,
        },
        WorkflowEdgeSpec {
            id: "triage-inspect".into(),
            source: "triage".into(),
            target: "inspect".into(),
            source_handle: None,
        },
        WorkflowEdgeSpec {
            id: "inspect-output".into(),
            source: "inspect".into(),
            target: "output".into(),
            source_handle: None,
        },
    ];
    contract
        .validate_graph_bindings(&workflow)
        .expect("read uses the preceding assignment");
}

#[test]
fn independent_variable_targets_each_begin_at_mutation_order_one() {
    let mut spec = contract_spec();
    let mut detail = declaration(
        "detail",
        WorkflowVariableScope::Run,
        WorkflowDataType::String,
        '8',
    );
    detail.mutation_mode = WorkflowVariableMutationMode::Deterministic;
    spec.declarations.push(detail);
    spec.assignments.push(WorkflowVariableAssignment {
        id: "assign-detail".into(),
        target_variable: "detail".into(),
        source_variable: "triage_result".into(),
        writer_step_id: "triage".into(),
        writer_region_id: None,
        source_path: vec!["detail".into()],
        value_type: WorkflowDataType::String,
        value_schema_digest: digest('8'),
        mutation_order: 1,
        expected_revision_variable: None,
        idempotency_key_variable: None,
    });
    assert!(WorkflowVariableContract::from_spec(spec).is_ok());
}

#[test]
fn one_consumer_port_cannot_receive_two_variable_reads() {
    let mut spec = contract_spec();
    let mut duplicate = spec
        .reads
        .iter()
        .find(|read| read.id == "output-summary")
        .expect("summary read")
        .clone();
    duplicate.id = "duplicate-target".into();
    duplicate.variable = "request".into();
    duplicate.path = vec!["summary".into()];
    duplicate.expected_schema_digest = digest('c');
    spec.reads.push(duplicate);
    assert!(WorkflowVariableContract::from_spec(spec).is_err());
}

#[test]
fn optional_node_output_reads_may_cross_but_not_reverse_a_branch() {
    let mut spec = contract_spec();
    spec.reads.push(WorkflowVariableRead {
        id: "output-triage-result".into(),
        variable: "triage_result".into(),
        consumer_step_id: "output".into(),
        consumer_region_id: None,
        target_port: "details".into(),
        path: Vec::new(),
        expected_type: WorkflowDataType::Object,
        expected_schema_digest: digest('b'),
        required: false,
        mode: WorkflowVariableReadMode::DirectValue,
    });
    let contract = WorkflowVariableContract::from_spec(spec).expect("contract");
    contract
        .validate_graph_bindings(&workflow_spec())
        .expect("optional forward read");

    let mut reversed = workflow_spec();
    reversed
        .edges
        .iter_mut()
        .find(|edge| edge.id == "input-triage")
        .expect("edge")
        .source = "output".into();
    assert!(contract.validate_graph_bindings(&reversed).is_err());
}

#[test]
fn branch_local_output_is_optional_at_a_join_and_never_required() {
    let mut spec = contract_spec();
    let result = spec
        .declarations
        .iter_mut()
        .find(|value| value.name == "triage_result")
        .expect("result");
    result.source_step_id = Some("left".into());
    spec.declarations
        .retain(|declaration| declaration.name == "triage_result");
    spec.reads.retain(|read| read.id == "output-summary");
    spec.assignments.clear();
    let read = spec
        .reads
        .iter_mut()
        .find(|value| value.id == "output-summary")
        .expect("join read");
    read.variable = "triage_result".into();
    read.expected_type = WorkflowDataType::Object;
    read.expected_schema_digest = digest('b');
    read.required = false;
    let optional = WorkflowVariableContract::from_spec(spec.clone()).expect("optional contract");
    optional
        .validate_graph_bindings(&branch_workflow_spec())
        .expect("optional branch value");

    spec.reads
        .iter_mut()
        .find(|value| value.id == "output-summary")
        .expect("join read")
        .required = true;
    let required = WorkflowVariableContract::from_spec(spec).expect("required contract");
    assert!(required
        .validate_graph_bindings(&branch_workflow_spec())
        .is_err());
}

fn branch_workflow_spec() -> WorkflowSpec {
    WorkflowSpec {
        name: "Branch variables".into(),
        description: String::new(),
        steps: vec![
            step("input", WorkflowStepKind::Input, 'a', 'a'),
            step("route", WorkflowStepKind::Branch, 'a', 'a'),
            step("left", WorkflowStepKind::Transform, 'a', 'b'),
            step("right", WorkflowStepKind::Transform, 'a', '8'),
            step("output", WorkflowStepKind::Output, 'c', 'c'),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-route".into(),
                source: "input".into(),
                target: "route".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "route-left".into(),
                source: "route".into(),
                target: "left".into(),
                source_handle: Some("left".into()),
            },
            WorkflowEdgeSpec {
                id: "route-right".into(),
                source: "route".into(),
                target: "right".into(),
                source_handle: Some("right".into()),
            },
            WorkflowEdgeSpec {
                id: "left-output".into(),
                source: "left".into(),
                target: "output".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "right-output".into(),
                source: "right".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    }
}
