use super::connector_response::parse_json_output;
use crate::modules::workflow::domain::{
    WorkflowDataField, WorkflowDataSchema, WorkflowDataType, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};

fn object_schema() -> WorkflowDataSchema {
    WorkflowDataSchema {
        value_type: WorkflowDataType::Object,
        fields: vec![WorkflowDataField {
            name: "accepted".into(),
            value_type: WorkflowDataType::Boolean,
            required: true,
        }],
    }
}

#[test]
fn connector_json_response_is_projected_through_the_exact_output_schema() {
    let output = parse_json_output(br#"{"accepted":true}"#, &object_schema())
        .expect("typed Connector output");
    assert_eq!(output, serde_json::json!({"accepted": true}));
}

#[test]
fn connector_json_response_rejects_invalid_duplicate_and_schema_drift_without_echoing_body() {
    for body in [
        br#"secret-provider-text"#.as_slice(),
        br#"{"accepted":true,"accepted":false}"#.as_slice(),
        br#"{"accepted":"secret-provider-text"}"#.as_slice(),
    ] {
        let error = parse_json_output(body, &object_schema()).expect_err("response must fail");
        assert!(!error.contains("secret-provider-text"), "{error}");
        assert!(!error.contains("accepted"), "{error}");
    }
}

#[test]
fn connector_json_response_cannot_expand_past_the_workflow_output_bound() {
    let body = serde_json::to_vec(&"x".repeat(WORKFLOW_RUN_OUTPUT_MAX_BYTES))
        .expect("oversized JSON string");
    let schema = WorkflowDataSchema {
        value_type: WorkflowDataType::String,
        fields: Vec::new(),
    };
    assert!(parse_json_output(&body, &schema).is_err());
}
