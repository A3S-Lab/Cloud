use super::*;
use a3s_cloud_contracts::{
    AutomationDefinitionV1, AutomationInvocationOriginV1, AutomationRevisionV1,
    AutomationWebhookSecretReferenceV1, AutomationWebhookSignatureAlgorithmV1,
    AutomationWebhookSignatureV1,
};
use chrono::{DateTime, Utc};
use serde_json::json;

const WEBHOOK_DEFINITION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/aut0.1/automation-definition-webhook.acl"
));

fn id(value: u16) -> Uuid {
    Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0000_u128 + u128::from(value))
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp")
        .with_timezone(&Utc)
}

fn revision() -> AutomationRevisionV1 {
    let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
    AutomationRevisionV1::from_definition(id(1), 1, None, definition.spec().clone())
        .expect("revision")
}

fn endpoint(revision: &AutomationRevisionV1) -> AutomationWebhookEndpointV1 {
    AutomationWebhookEndpointV1::for_revision(
        id(2),
        "release-hook",
        AutomationWebhookSecretReferenceV1 {
            secret_id: id(3),
            version: 4,
        },
        4_096,
        revision,
        timestamp("2026-09-05T00:00:00.000Z"),
    )
    .expect("endpoint")
}

fn request(
    endpoint: &AutomationWebhookEndpointV1,
    delivery_id: Uuid,
    received_at: DateTime<Utc>,
) -> AutomationWebhookRequestV1 {
    AutomationWebhookRequestV1::from_json(
        endpoint,
        delivery_id,
        AutomationWebhookSignatureV1 {
            algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
            key_version: endpoint.signing_secret.version,
            value: format!("hmac-sha256:{}", "a".repeat(64)),
        },
        "application/json",
        br#"{"release":"stable"}"#,
        received_at,
    )
    .expect("request")
}

fn authorization(revision: &AutomationRevisionV1) -> AutomationInvocationAuthorizationV1 {
    AutomationInvocationAuthorizationV1 {
        policy_digest: revision
            .spec()
            .definition
            .authorization
            .policy_digest
            .clone(),
        grant_snapshot_digest: format!("sha256:{}", "b".repeat(64)),
        principal_id: Some(id(4)),
    }
}

#[test]
fn builds_exact_webhook_invocation_from_captured_request() {
    let revision = revision();
    let endpoint = endpoint(&revision);
    let received_at = timestamp("2026-09-05T00:00:01.000Z");
    let request = request(&endpoint, id(5), received_at);
    let envelope = AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
        endpoint: &endpoint,
        revision: &revision,
        request: &request,
        invocation_id: id(6),
        requested_at: timestamp("2026-09-05T00:00:02.000Z"),
        authorization: authorization(&revision),
        correlation_id: id(7),
        causation_id: Some(id(8)),
    })
    .expect("invocation");

    assert_eq!(envelope.invocation_id, id(6));
    assert_eq!(envelope.subscription, Some(endpoint.subscription.clone()));
    assert!(envelope.deduplication_key.contains("subscription/"));
    assert!(matches!(
        envelope.origin,
        AutomationInvocationOriginV1::Event {
            event_id,
            event_key,
            observed_at,
            ..
        } if event_id == id(5)
            && event_key == AUTOMATION_WEBHOOK_EVENT_KEY
            && observed_at == received_at
    ));
    assert_eq!(
        envelope.input,
        AutomationInvocationInputV1::inline_json(json!({"release": "stable"})).expect("input")
    );
}

#[test]
fn rejects_request_time_before_capture() {
    let revision = revision();
    let endpoint = endpoint(&revision);
    let request = request(&endpoint, id(9), timestamp("2026-09-05T00:00:02.000Z"));
    let error = AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
        endpoint: &endpoint,
        revision: &revision,
        request: &request,
        invocation_id: id(10),
        requested_at: timestamp("2026-09-05T00:00:01.000Z"),
        authorization: authorization(&revision),
        correlation_id: id(11),
        causation_id: None,
    })
    .expect_err("ordering error");
    assert_eq!(
        error,
        "Automation webhook invocation cannot be requested before request receipt"
    );
}

#[test]
fn rejects_endpoint_or_revision_drift() {
    let revision = revision();
    let endpoint = endpoint(&revision);
    let request = request(&endpoint, id(12), timestamp("2026-09-05T00:00:01.000Z"));
    let other_revision = AutomationRevisionV1::from_definition(
        id(13),
        1,
        None,
        AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION)
            .expect("definition")
            .spec()
            .clone(),
    )
    .expect("other revision");
    assert!(
        AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
            endpoint: &endpoint,
            revision: &other_revision,
            request: &request,
            invocation_id: id(14),
            requested_at: timestamp("2026-09-05T00:00:02.000Z"),
            authorization: authorization(&other_revision),
            correlation_id: id(15),
            causation_id: None,
        })
        .is_err()
    );
}

#[test]
fn replays_the_exact_envelope_and_policy_deduplication_key() {
    let revision = revision();
    let endpoint = endpoint(&revision);
    let received_at = timestamp("2026-09-05T00:00:01.000Z");
    let request = request(&endpoint, id(16), received_at);
    let build = |invocation_id| {
        AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
            endpoint: &endpoint,
            revision: &revision,
            request: &request,
            invocation_id,
            requested_at: received_at,
            authorization: authorization(&revision),
            correlation_id: id(17),
            causation_id: Some(id(18)),
        })
        .expect("replayed invocation")
    };
    let first = build(id(19));
    assert_eq!(first, build(id(19)));
    assert_eq!(first.deduplication_key, build(id(20)).deduplication_key);
    assert_eq!(first.correlation_id, id(17));
    assert_eq!(first.causation_id, Some(id(18)));
}

#[test]
fn rejects_authorization_policy_drift() {
    let revision = revision();
    let endpoint = endpoint(&revision);
    let received_at = timestamp("2026-09-05T00:00:01.000Z");
    let request = request(&endpoint, id(21), received_at);
    let mut authorization = authorization(&revision);
    authorization.policy_digest = format!("sha256:{}", "f".repeat(64));
    let error = AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
        endpoint: &endpoint,
        revision: &revision,
        request: &request,
        invocation_id: id(22),
        requested_at: received_at,
        authorization,
        correlation_id: id(23),
        causation_id: None,
    })
    .expect_err("authorization drift");
    assert_eq!(error, "Automation invocation authorization policy drifted");
}

#[test]
fn rejects_captured_request_identity_and_content_drift() {
    let revision = revision();
    let endpoint = endpoint(&revision);
    let received_at = timestamp("2026-09-05T00:00:01.000Z");
    let request = request(&endpoint, id(24), received_at);
    let mut wrong_endpoint = request.clone();
    wrong_endpoint.endpoint_id = id(25);
    let mut wrong_digest = request.clone();
    wrong_digest.body_digest = format!("sha256:{}", "f".repeat(64));
    let mut wrong_payload = request;
    wrong_payload.payload = json!({"release": "untrusted"});
    for drifted in [wrong_endpoint, wrong_digest, wrong_payload] {
        assert!(
            AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
                endpoint: &endpoint,
                revision: &revision,
                request: &drifted,
                invocation_id: id(26),
                requested_at: received_at,
                authorization: authorization(&revision),
                correlation_id: id(27),
                causation_id: None,
            })
            .is_err()
        );
    }
}
