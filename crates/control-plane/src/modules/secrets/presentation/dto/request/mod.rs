mod create_secret_request;
mod secret_value_request;

pub use create_secret_request::CreateSecretRequest;
pub use secret_value_request::SecretValueRequest;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_request_debug_output_redacts_values() {
        let create = CreateSecretRequest {
            name: "Database URL".into(),
            value: "do-not-log-create".into(),
        };
        let rotate = SecretValueRequest {
            value: "do-not-log-rotate".into(),
        };
        let debug = format!("{create:?} {rotate:?}");
        assert!(debug.contains("<redacted-secret-plaintext>"));
        assert!(!debug.contains("do-not-log-create"));
        assert!(!debug.contains("do-not-log-rotate"));
    }
}
