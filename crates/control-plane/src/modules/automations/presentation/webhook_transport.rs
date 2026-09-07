use crate::modules::automations::application::{
    AutomationWebhookEndpointScope, ReceiveAutomationWebhookDelivery,
};
use a3s_cloud_contracts::{
    AutomationWebhookSignatureAlgorithmV1, AutomationWebhookSignatureV1,
    AUTOMATION_WEBHOOK_MAX_BODY_BYTES,
};
use chrono::{DateTime, Timelike, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

const DELIVERY_ID_HEADER: &str = "x-a3s-delivery-id";
const SIGNATURE_HEADER: &str = "x-a3s-signature";
const KEY_VERSION_HEADER: &str = "x-a3s-key-version";
const CONTENT_TYPE_HEADER: &str = "content-type";
const CAUSATION_ID_HEADER: &str = "x-a3s-causation-id";

/// Framework-neutral signed webhook request extracted by a listener or
/// Gateway adapter. The adapter derives internal identities from the required
/// delivery ID, so transport input cannot choose a grant or an invocation
/// identity independently of the delivery it names.
#[derive(Debug, Clone)]
pub struct AutomationWebhookTransportRequest {
    pub scope: AutomationWebhookEndpointScope,
    pub endpoint_key: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub received_at: DateTime<Utc>,
}

impl AutomationWebhookTransportRequest {
    pub const fn max_body_bytes() -> u64 {
        AUTOMATION_WEBHOOK_MAX_BODY_BYTES
    }

    pub fn into_receive_command(self) -> Result<ReceiveAutomationWebhookDelivery, String> {
        if self.body.len() > AUTOMATION_WEBHOOK_MAX_BODY_BYTES as usize {
            return Err("Automation webhook transport body exceeds its bound".into());
        }
        let headers = normalized_headers(self.headers)?;
        let delivery_id = parse_uuid(required(&headers, DELIVERY_ID_HEADER)?, DELIVERY_ID_HEADER)?;
        let signature = parse_signature(
            required(&headers, SIGNATURE_HEADER)?,
            required(&headers, KEY_VERSION_HEADER)?,
        )?;
        let content_type = required(&headers, CONTENT_TYPE_HEADER)?.to_owned();
        let causation_id = headers
            .get(CAUSATION_ID_HEADER)
            .map(|value| parse_uuid(value, CAUSATION_ID_HEADER))
            .transpose()?;
        let received_at = canonical_millisecond(self.received_at)?;
        Ok(ReceiveAutomationWebhookDelivery {
            scope: self.scope,
            endpoint_key: self.endpoint_key,
            delivery_id,
            signature,
            content_type,
            body: self.body,
            received_at,
            invocation_id: Uuid::new_v5(&delivery_id, b"a3s.automation.webhook.invocation.v1"),
            requested_at: received_at,
            correlation_id: Uuid::new_v5(&delivery_id, b"a3s.automation.webhook.correlation.v1"),
            causation_id,
            receipt_id: Uuid::new_v5(&delivery_id, b"a3s.automation.webhook.receipt.v1"),
            recorded_at: received_at,
        })
    }
}

fn normalized_headers(headers: Vec<(String, String)>) -> Result<BTreeMap<String, String>, String> {
    let mut normalized = BTreeMap::new();
    for (name, value) in headers {
        if name.is_empty() || name.trim() != name || value.trim() != value {
            return Err("Automation webhook transport header is not canonical".into());
        }
        let name = name.to_ascii_lowercase();
        if normalized.insert(name.clone(), value).is_some() {
            return Err(format!(
                "Automation webhook transport header is duplicated: {name}"
            ));
        }
    }
    Ok(normalized)
}

fn required<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    headers
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Automation webhook transport header is missing: {name}"))
}

fn parse_uuid(value: &str, label: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| format!("Automation webhook {label} is not a UUID"))
}

fn parse_signature(value: &str, key_version: &str) -> Result<AutomationWebhookSignatureV1, String> {
    let key_version = key_version
        .parse::<u64>()
        .map_err(|_| "Automation webhook key version is not an integer".to_owned())?;
    let signature = AutomationWebhookSignatureV1 {
        algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
        key_version,
        value: value.to_owned(),
    };
    signature.validate()?;
    Ok(signature)
}

fn canonical_millisecond(value: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    value
        .with_nanosecond(value.timestamp_subsec_millis() * 1_000_000)
        .ok_or_else(|| "Automation webhook transport timestamp is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn request() -> AutomationWebhookTransportRequest {
        AutomationWebhookTransportRequest {
            scope: AutomationWebhookEndpointScope {
                organization_id: Uuid::from_u128(1),
                project_id: Uuid::from_u128(2),
                environment_id: Uuid::from_u128(3),
            },
            endpoint_key: "release-hook".into(),
            headers: vec![
                (
                    DELIVERY_ID_HEADER.into(),
                    "018f0000-0000-7000-8000-000000000501".into(),
                ),
                (
                    SIGNATURE_HEADER.into(),
                    format!("hmac-sha256:{}", "a".repeat(64)),
                ),
                (KEY_VERSION_HEADER.into(), "4".into()),
                (CONTENT_TYPE_HEADER.into(), "application/json".into()),
            ],
            body: br#"{"release":"stable"}"#.to_vec(),
            received_at: Utc.timestamp_nanos(1_700_000_000_123_456_789),
        }
    }

    #[test]
    fn parses_signed_transport_and_derives_stable_internal_identities() {
        let first = request().into_receive_command().expect("transport command");
        let second = request()
            .into_receive_command()
            .expect("transport command replay");
        assert_eq!(first.delivery_id, second.delivery_id);
        assert_eq!(first.invocation_id, second.invocation_id);
        assert_eq!(first.receipt_id, second.receipt_id);
        assert_eq!(first.received_at.timestamp_subsec_nanos(), 123_000_000);
        assert_eq!(first.signature.key_version, 4);
    }

    #[test]
    fn header_names_are_case_insensitive_but_duplicates_fail_closed() {
        let mut case_variant = request();
        case_variant.headers[0].0 = "X-A3S-DELIVERY-ID".into();
        assert!(case_variant.into_receive_command().is_ok());

        let mut duplicate = request();
        duplicate.headers.push((
            DELIVERY_ID_HEADER.into(),
            "018f0000-0000-7000-8000-000000000502".into(),
        ));
        assert!(duplicate.into_receive_command().is_err());
    }

    #[test]
    fn malformed_signature_id_and_body_bounds_are_rejected() {
        let mut signature = request();
        signature.headers[1].1 = "hmac-sha256:bad".into();
        assert!(signature.into_receive_command().is_err());

        let mut delivery = request();
        delivery.headers[0].1 = "not-a-uuid".into();
        assert!(delivery.into_receive_command().is_err());

        let mut body = request();
        body.body = vec![b'x'; AUTOMATION_WEBHOOK_MAX_BODY_BYTES as usize + 1];
        assert!(body.into_receive_command().is_err());
    }
}
