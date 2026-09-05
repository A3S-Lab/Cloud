use crate::modules::automations::domain::IAutomationWebhookSignatureVerifier;
use crate::modules::secrets::application::IExactSecretMaterializer;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SecretId, SecretVersionReference,
};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Verifies the canonical `hmac-sha256:<lowercase-hex>` fact over the captured
/// raw request body.
///
/// Secret lookup and decryption stay behind Secrets' exact-version materializer.
/// This adapter receives the plaintext only for the comparison and never stores
/// or returns it. The materializer's `SecretPlaintext` zeroizes on drop.
pub struct HmacSha256AutomationWebhookSignatureVerifier {
    materializer: Arc<dyn IExactSecretMaterializer>,
}

impl HmacSha256AutomationWebhookSignatureVerifier {
    pub fn new(materializer: Arc<dyn IExactSecretMaterializer>) -> Self {
        Self { materializer }
    }
}

#[async_trait]
impl IAutomationWebhookSignatureVerifier for HmacSha256AutomationWebhookSignatureVerifier {
    async fn verify(
        &self,
        endpoint: &a3s_cloud_contracts::AutomationWebhookEndpointV1,
        request: &a3s_cloud_contracts::AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        endpoint.validate().map_err(|error| {
            format!("Automation webhook endpoint is invalid for signature verification: {error}")
        })?;
        request
            .validate_for_endpoint(endpoint)
            .map_err(|error| format!("Automation webhook request is invalid: {error}"))?;

        let secret_reference = SecretVersionReference::new(
            SecretId::from_uuid(endpoint.signing_secret.secret_id),
            endpoint.signing_secret.version,
        )
        .map_err(|error| format!("Automation webhook Secret reference is invalid: {error}"))?;
        let secret = self
            .materializer
            .materialize_reference(
                OrganizationId::from_uuid(endpoint.organization_id),
                ProjectId::from_uuid(endpoint.project_id),
                EnvironmentId::from_uuid(endpoint.environment_id),
                secret_reference,
            )
            .await
            .map_err(secret_materialization_error)?;

        let body = STANDARD
            .decode(&request.body_base64)
            .map_err(|_| "Automation webhook captured body cannot be decoded".to_owned())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|_| "Automation webhook Secret cannot initialize HMAC".to_owned())?;
        mac.update(&body);
        let expected = format!("hmac-sha256:{:x}", mac.finalize().into_bytes());
        if expected
            .as_bytes()
            .ct_eq(request.signature.value.as_bytes())
            .unwrap_u8()
            != 1
        {
            return Err("Automation webhook signature verification failed".into());
        }
        Ok(())
    }
}

fn secret_materialization_error(error: ApplicationError) -> String {
    match error {
        ApplicationError::Forbidden(_)
        | ApplicationError::NotFound(_)
        | ApplicationError::Invalid(_)
        | ApplicationError::Conflict(_)
        | ApplicationError::Unavailable(_)
        | ApplicationError::Internal(_) => {
            "Automation webhook signing Secret is unavailable".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::secrets::application::SecretPlaintext;
    use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationRevisionV1, AutomationWebhookEndpointV1,
        AutomationWebhookRequestV1, AutomationWebhookSecretReferenceV1,
        AutomationWebhookSignatureAlgorithmV1, AutomationWebhookSignatureV1,
    };
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use hmac::{Hmac, Mac};
    use std::sync::Mutex;
    use uuid::Uuid;

    const WEBHOOK_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    struct FixedMaterializer {
        value: Vec<u8>,
        references: Mutex<Vec<SecretVersionReference>>,
    }

    #[async_trait]
    impl IExactSecretMaterializer for FixedMaterializer {
        async fn materialize_reference(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
            reference: SecretVersionReference,
        ) -> ApplicationResult<SecretPlaintext> {
            self.references
                .lock()
                .expect("reference lock")
                .push(reference);
            SecretPlaintext::new(self.value.clone()).map_err(ApplicationError::Internal)
        }
    }

    struct FailingMaterializer;

    #[async_trait]
    impl IExactSecretMaterializer for FailingMaterializer {
        async fn materialize_reference(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
            _reference: SecretVersionReference,
        ) -> ApplicationResult<SecretPlaintext> {
            Err(ApplicationError::Forbidden("fixture denial".into()))
        }
    }

    fn id(value: u16) -> Uuid {
        Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0000_u128 + u128::from(value))
    }

    fn timestamp(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }

    fn endpoint() -> (AutomationWebhookEndpointV1, AutomationRevisionV1) {
        let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
        let revision =
            AutomationRevisionV1::from_definition(id(0x100), 1, None, definition.spec().clone())
                .expect("revision");
        let endpoint = AutomationWebhookEndpointV1::for_revision(
            id(0x101),
            "release-hook",
            AutomationWebhookSecretReferenceV1 {
                secret_id: id(0x102),
                version: 4,
            },
            4096,
            &revision,
            timestamp("2026-09-05T00:00:00.000Z"),
        )
        .expect("endpoint");
        (endpoint, revision)
    }

    fn request(endpoint: &AutomationWebhookEndpointV1) -> AutomationWebhookRequestV1 {
        AutomationWebhookRequestV1::from_json(
            endpoint,
            id(0x401),
            AutomationWebhookSignatureV1 {
                algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
                key_version: endpoint.signing_secret.version,
                value: format!("hmac-sha256:{}", "0".repeat(64)),
            },
            "application/json",
            br#"{"release":"stable"}"#,
            timestamp("2026-09-05T00:00:01.000Z"),
        )
        .expect("request")
    }

    fn sign(request: &AutomationWebhookRequestV1, secret: &[u8]) -> String {
        let body = STANDARD.decode(&request.body_base64).expect("body");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC");
        mac.update(&body);
        format!("hmac-sha256:{:x}", mac.finalize().into_bytes())
    }

    #[tokio::test]
    async fn verifies_raw_body_with_exact_secret_reference_and_constant_time_fact() {
        let (endpoint, _) = endpoint();
        let mut request = request(&endpoint);
        let secret = b"webhook-secret-v4";
        request.signature.value = sign(&request, secret);
        let materializer = Arc::new(FixedMaterializer {
            value: secret.to_vec(),
            references: Mutex::new(Vec::new()),
        });
        let verifier = HmacSha256AutomationWebhookSignatureVerifier::new(materializer.clone());

        verifier
            .verify(&endpoint, &request)
            .await
            .expect("signature");
        assert_eq!(
            materializer
                .references
                .lock()
                .expect("reference lock")
                .as_slice(),
            &[SecretVersionReference::new(SecretId::from_uuid(id(0x102)), 4).expect("reference")]
        );
    }

    #[tokio::test]
    async fn rejects_tampering_and_does_not_expose_materialization_errors() {
        let (endpoint, _) = endpoint();
        let base_request = request(&endpoint);
        let verifier =
            HmacSha256AutomationWebhookSignatureVerifier::new(Arc::new(FailingMaterializer));
        let error = verifier
            .verify(&endpoint, &base_request)
            .await
            .expect_err("denial");
        assert_eq!(error, "Automation webhook signing Secret is unavailable");
        assert!(!error.contains("fixture"));

        let materializer = Arc::new(FixedMaterializer {
            value: b"webhook-secret-v4".to_vec(),
            references: Mutex::new(Vec::new()),
        });
        let verifier = HmacSha256AutomationWebhookSignatureVerifier::new(materializer.clone());
        let mut wrong_signature = request(&endpoint);
        wrong_signature.signature.value = format!("hmac-sha256:{}", "1".repeat(64));
        let error = verifier
            .verify(&endpoint, &wrong_signature)
            .await
            .expect_err("wrong signature");
        assert_eq!(error, "Automation webhook signature verification failed");

        let mut signed = base_request;
        signed.signature.value = sign(&signed, b"webhook-secret-v4");
        signed.body_base64 = STANDARD.encode(br#"{"release":"tampered"}"#);
        let error = verifier
            .verify(&endpoint, &signed)
            .await
            .expect_err("tampering");
        assert!(error.contains("request is invalid"));
    }
}
