use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestRecipientContactVerificationRequest {
    pub address: String,
}

impl std::fmt::Debug for RequestRecipientContactVerificationRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RequestRecipientContactVerificationRequest")
            .field("address", &"[REDACTED]")
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteRecipientContactVerificationRequest {
    pub proof: String,
}

impl std::fmt::Debug for CompleteRecipientContactVerificationRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompleteRecipientContactVerificationRequest")
            .field("proof", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeRecipientContactRequest {
    pub expected_version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_request_debug_output_is_redacted_and_schemas_are_closed() {
        let address: RequestRecipientContactVerificationRequest =
            serde_json::from_str(r#"{"address":"private@example.test"}"#).expect("address request");
        assert!(!format!("{address:?}").contains("private@example.test"));

        let proof: CompleteRecipientContactVerificationRequest =
            serde_json::from_str(r#"{"proof":"a3srcv1.private.proof"}"#).expect("proof request");
        assert!(!format!("{proof:?}").contains("a3srcv1.private.proof"));

        assert!(serde_json::from_str::<RequestRecipientContactVerificationRequest>(
            r#"{"address":"private@example.test","principalId":"00000000-0000-0000-0000-000000000000"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<CompleteRecipientContactVerificationRequest>(
            r#"{"proof":"a3srcv1.private.proof","contactId":"00000000-0000-0000-0000-000000000000"}"#
        )
        .is_err());
    }
}
