use super::*;
use crate::modules::shared_kernel::domain::Sha256Digest;

const DEFAULTS_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.3/variable-defaults.acl"
));

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn contract(default: &WorkflowVariableDefault) -> WorkflowVariableContract {
    WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.defaults".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        declarations: vec![WorkflowVariableDeclaration {
            name: default.name.clone(),
            scope: WorkflowVariableScope::Run,
            value_type: WorkflowDataType::Object,
            value_schema_digest: digest('a'),
            source_schema_digest: None,
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Deterministic,
            required: false,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: None,
            default_value_digest: Some(default.digest.clone()),
        }],
        reads: Vec::new(),
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .expect("variable contract")
}

#[test]
fn defaults_are_canonical_digest_addressed_and_contract_bound() {
    let default = WorkflowVariableDefault::new(
        "fallback",
        serde_json::json!({"priority": "normal", "labels": ["support"]}),
    )
    .expect("default");
    let contract = contract(&default);
    let defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: "support.defaults".into(),
        revision: "1.0.0".into(),
        values: vec![default.clone()],
    })
    .expect("defaults");

    defaults
        .validate_contract(&contract)
        .expect("matching contract");
    assert_eq!(defaults.value("fallback"), Some(&default.value));
    assert!(defaults
        .canonical_acl()
        .contains("schema = \"cloud.workflow.variable-defaults.v1\""));
    assert_eq!(
        WorkflowVariableDefaults::restore(defaults.canonical_acl(), defaults.digest().as_str())
            .expect("restored defaults"),
        defaults
    );
    let crlf = defaults.canonical_acl().replace('\n', "\r\n");
    assert_eq!(
        WorkflowVariableDefaults::parse_acl(&crlf).expect("canonical CRLF input"),
        defaults
    );
}

#[test]
fn checked_in_default_material_is_canonical_and_exact() {
    let defaults = WorkflowVariableDefaults::parse_acl(DEFAULTS_FIXTURE)
        .expect("checked-in variable defaults");
    assert_eq!(defaults.spec().id, "support.defaults");
    assert_eq!(
        defaults.value("fallback"),
        Some(&serde_json::json!("normal"))
    );
    assert_eq!(
        defaults.spec().values[0].digest.to_string(),
        "sha256:82fbb169e798324839513347c048ecc9c91a6574588e1760f13d6b9650c328bf"
    );
}

#[test]
fn defaults_fail_closed_on_value_digest_type_and_coverage_drift() {
    let default =
        WorkflowVariableDefault::new("fallback", serde_json::json!({"priority": "normal"}))
            .expect("default");
    let contract = contract(&default);

    let mut digest_drift = default.clone();
    digest_drift.digest = digest('f');
    assert!(
        WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
            id: "support.defaults".into(),
            revision: "1.0.0".into(),
            values: vec![digest_drift],
        })
        .is_err()
    );

    let wrong_type = WorkflowVariableDefault::new("fallback", serde_json::json!("normal"))
        .expect("string default");
    let wrong_type_defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: "support.defaults".into(),
        revision: "1.0.0".into(),
        values: vec![wrong_type],
    })
    .expect("self-consistent defaults");
    assert!(wrong_type_defaults.validate_contract(&contract).is_err());

    let extra = WorkflowVariableDefault::new("extra", serde_json::json!({})).expect("extra");
    let extra_defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: "support.defaults".into(),
        revision: "1.0.0".into(),
        values: vec![default, extra],
    })
    .expect("self-consistent defaults");
    assert!(extra_defaults.validate_contract(&contract).is_err());
}

#[test]
fn variable_defaults_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowVariableDefault>();
    assert_send_sync::<WorkflowVariableDefaultsSpec>();
    assert_send_sync::<WorkflowVariableDefaults>();
}
