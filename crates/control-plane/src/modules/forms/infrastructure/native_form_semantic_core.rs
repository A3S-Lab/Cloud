use crate::modules::forms::domain::{FormSemanticCoreError, IFormSemanticCore};
use a3s_form_core::{compile_bytes, evaluate_bytes, COMPILER_REVISION};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeFormSemanticCore;

impl NativeFormSemanticCore {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl IFormSemanticCore for NativeFormSemanticCore {
    fn compiler_revision(&self) -> &'static str {
        COMPILER_REVISION
    }

    fn compile(&self, request: &[u8]) -> Result<Vec<u8>, FormSemanticCoreError> {
        compile_bytes(request)
            .map_err(|error| FormSemanticCoreError::Compilation(error.to_string()))
    }

    fn evaluate(&self, request: &[u8]) -> Result<Vec<u8>, FormSemanticCoreError> {
        evaluate_bytes(request)
            .map_err(|error| FormSemanticCoreError::Evaluation(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct EvaluationConformanceFixture {
        api_version: String,
        cases: Vec<EvaluationCase>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct EvaluationCase {
        name: String,
        request: serde_json::Value,
        response: serde_json::Value,
    }

    #[test]
    fn consumes_the_owner_value_evaluation_golden_corpus_byte_for_byte() {
        let fixture: EvaluationConformanceFixture = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/form-value-evaluation-v1.json"
        )))
        .expect("shared Form value-evaluation fixture should decode");
        assert_eq!(
            fixture.api_version,
            "a3s.dev/form-value-evaluation-conformance/v1"
        );

        let core = NativeFormSemanticCore::new();
        assert_eq!(core.compiler_revision(), "a3s-form-core@0.1.0");
        for case in fixture.cases {
            let request = canonical_bytes(&case.request);
            let expected = canonical_bytes(&case.response);
            let actual = core
                .evaluate(&request)
                .unwrap_or_else(|error| panic!("{} evaluation failed: {error}", case.name));
            assert_eq!(actual, expected, "{}", case.name);
        }
    }

    #[test]
    fn preserves_owner_protocol_failures_instead_of_falling_back() {
        let core = NativeFormSemanticCore::new();
        let response = core
            .evaluate(br#"{"apiVersion":"unsupported","formPlan":{},"value":{}}"#)
            .expect("protocol failure should use the owner response envelope");
        let response: serde_json::Value =
            serde_json::from_slice(&response).expect("response should decode");
        assert_eq!(response["ok"], false);
        assert_eq!(response["errors"][0]["code"], "protocol.api_version");
    }

    fn canonical_bytes(value: &serde_json::Value) -> Vec<u8> {
        let encoded = serde_json::to_vec(value).expect("fixture value should encode");
        a3s_form_core::canonicalize_json(&encoded).expect("fixture value should canonicalize")
    }
}
