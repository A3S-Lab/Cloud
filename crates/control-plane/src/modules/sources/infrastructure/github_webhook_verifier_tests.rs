use super::{GithubWebhookVerifier, HmacSha256};
use crate::modules::sources::domain::{
    ISourceWebhookVerifier, SourceWebhookVerificationError, SourceWebhookVerificationRequest,
    VerifiedRepositoryWebhook, VerifiedSourceWebhook,
};
use hmac::Mac;
use serde_json::{json, Value};

const SECRET: &str = "github-webhook-test-secret-0123456789abcdef";

#[test]
fn verifies_exact_raw_push_and_returns_typed_identity() {
    let verifier = verifier(1024 * 1024);
    let body = payload();
    let signature = signature(SECRET, &body);
    let verified = verifier
        .verify(request("push", "delivery-1", &signature, &body))
        .expect("signed webhook");
    let VerifiedSourceWebhook::Repository(VerifiedRepositoryWebhook::Push(push)) = verified else {
        panic!("expected push");
    };
    assert_eq!(
        push.repository.canonical_url(),
        "https://github.com/a3s-lab/cloud"
    );
    assert_eq!(push.installation_id.as_u64(), 42);
    assert_eq!(push.reference.value(), "main");
    assert_eq!(
        push.commit_sha.as_str(),
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(push.payload_digest.len(), 71);
}

#[test]
fn authenticates_before_parsing_and_rejects_noncanonical_signatures() {
    let verifier = verifier(1024 * 1024);
    let invalid_json = b"{".to_vec();
    let wrong = signature("another-webhook-secret-0123456789abcdef", &invalid_json);
    assert!(matches!(
        verifier.verify(request("push", "delivery-1", &wrong, &invalid_json)),
        Err(SourceWebhookVerificationError::Authentication)
    ));
    let uppercase = signature(SECRET, &payload()).to_ascii_uppercase();
    assert!(matches!(
        verifier.verify(request("push", "delivery-1", &uppercase, &payload())),
        Err(SourceWebhookVerificationError::Authentication)
    ));
    let signed = signature(SECRET, &invalid_json);
    assert!(matches!(
        verifier.verify(request("push", "delivery-1", &signed, &invalid_json)),
        Err(SourceWebhookVerificationError::Invalid(_))
    ));
}

#[test]
fn ignores_authenticated_non_push_and_deleted_push_events() {
    let verifier = verifier(1024 * 1024);
    let body = payload();
    let ping_signature = signature(SECRET, &body);
    assert!(matches!(
        verifier
            .verify(request("ping", "delivery-ping", &ping_signature, &body))
            .expect("signed ping"),
        VerifiedSourceWebhook::Ignored
    ));

    let mut deleted: Value = serde_json::from_slice(&body).expect("payload JSON");
    deleted["deleted"] = Value::Bool(true);
    deleted["after"] = Value::String("0000000000000000000000000000000000000000".into());
    let deleted = serde_json::to_vec(&deleted).expect("deleted payload");
    let signature = signature(SECRET, &deleted);
    assert!(matches!(
        verifier
            .verify(request("push", "delivery-delete", &signature, &deleted))
            .expect("signed deletion"),
        VerifiedSourceWebhook::Ignored
    ));
}

#[test]
fn verifies_installation_and_account_lifecycle_events_as_typed_changes() {
    let verifier = verifier(1024 * 1024);
    for (event, delivery, body, expected_action) in [
        (
            "installation",
            "delivery-suspend",
            json!({
                "action": "suspend",
                "installation": {
                    "id": 42,
                    "account": {"id": 100, "login": "A3S-Lab", "type": "Organization"}
                },
                "sender": {"id": 200, "login": "octocat"}
            }),
            "suspend",
        ),
        (
            "installation_target",
            "delivery-rename",
            json!({
                "action": "renamed",
                "installation": {"id": 42},
                "account": {"id": 100, "login": "A3S-Platform", "type": "Organization"},
                "changes": {"login": {"from": "A3S-Lab"}},
                "target_type": "Organization"
            }),
            "renamed",
        ),
        (
            "github_app_authorization",
            "delivery-revoked",
            json!({
                "action": "revoked",
                "sender": {"id": 200, "login": "octocat"}
            }),
            "revoked",
        ),
    ] {
        let body = serde_json::to_vec(&body).expect("lifecycle payload");
        let signature = signature(SECRET, &body);
        let verified = verifier
            .verify(request(event, delivery, &signature, &body))
            .expect("signed lifecycle webhook");
        let VerifiedSourceWebhook::GithubConnectionLifecycle(lifecycle) = verified else {
            panic!("expected connection lifecycle");
        };
        assert_eq!(lifecycle.delivery_id.as_str(), delivery);
        assert_eq!(lifecycle.change.event_name(), event);
        assert_eq!(lifecycle.change.action_name(), expected_action);
        assert_eq!(lifecycle.payload_digest.len(), 71);
    }
}

#[test]
fn verifies_exact_pull_request_lifecycle_and_fork_trust_boundary() {
    let verifier = verifier(1024 * 1024);
    let body = pull_request_payload(
        "synchronize",
        "open",
        false,
        Some(json!({
            "full_name": "contributor/cloud",
            "html_url": "https://github.com/contributor/cloud"
        })),
    );
    let signature = signature(SECRET, &body);
    let verified = verifier
        .verify(request(
            "pull_request",
            "delivery-pr-sync",
            &signature,
            &body,
        ))
        .expect("signed pull-request webhook");
    let VerifiedSourceWebhook::Repository(VerifiedRepositoryWebhook::PullRequest(change)) =
        verified
    else {
        panic!("expected typed pull-request change");
    };
    change.validate().expect("verified PR change");
    assert_eq!(change.pull_request_id, 1_000_042);
    assert_eq!(change.pull_request_number, 42);
    assert_eq!(change.base_reference.value(), "main");
    assert_eq!(change.head_reference.value(), "feature/preview");
    assert_eq!(
        change.head_commit_sha.as_str(),
        "fedcba9876543210fedcba9876543210fedcba98"
    );
    assert!(change.is_fork());
    assert_eq!(change.payload_digest.len(), 71);
    assert_eq!(
        change.provider_created_at,
        "2026-08-24T04:30:00Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .expect("provider creation time")
    );
    assert_eq!(
        change.provider_updated_at.timestamp_subsec_nanos(),
        123_456_000
    );
}

#[test]
fn closes_deleted_fork_heads_but_rejects_confused_pull_request_identity() {
    let verifier = verifier(1024 * 1024);
    let closed = pull_request_payload("closed", "closed", false, None);
    let closed_signature = signature(SECRET, &closed);
    let VerifiedSourceWebhook::Repository(VerifiedRepositoryWebhook::PullRequest(change)) =
        verifier
            .verify(request(
                "pull_request",
                "delivery-pr-close",
                &closed_signature,
                &closed,
            ))
            .expect("signed closed pull-request webhook")
    else {
        panic!("expected typed pull-request change");
    };
    assert!(change.head_repository.is_none());
    assert!(change.is_fork());

    let mut confused: Value = serde_json::from_slice(&pull_request_payload(
        "opened",
        "open",
        false,
        Some(json!({
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        })),
    ))
    .expect("pull-request JSON");
    confused["pull_request"]["base"]["repo"]["full_name"] = Value::String("A3S-Lab/Runtime".into());
    let confused = serde_json::to_vec(&confused).expect("confused PR payload");
    let confused_signature = signature(SECRET, &confused);
    assert!(matches!(
        verifier.verify(request(
            "pull_request",
            "delivery-pr-confused",
            &confused_signature,
            &confused,
        )),
        Err(SourceWebhookVerificationError::Invalid(_))
    ));

    let mut reversed_time: Value = serde_json::from_slice(&pull_request_payload(
        "opened",
        "open",
        false,
        Some(json!({
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        })),
    ))
    .expect("pull-request JSON");
    reversed_time["pull_request"]["created_at"] = Value::String("2026-08-24T06:30:00Z".into());
    let reversed_time = serde_json::to_vec(&reversed_time).expect("reversed-time PR payload");
    let reversed_time_signature = signature(SECRET, &reversed_time);
    assert!(matches!(
        verifier.verify(request(
            "pull_request",
            "delivery-pr-reversed-time",
            &reversed_time_signature,
            &reversed_time,
        )),
        Err(SourceWebhookVerificationError::Invalid(_))
    ));

    let ignored = pull_request_payload("edited", "open", false, None);
    let ignored_signature = signature(SECRET, &ignored);
    assert!(matches!(
        verifier
            .verify(request(
                "pull_request",
                "delivery-pr-edited",
                &ignored_signature,
                &ignored,
            ))
            .expect("signed unsupported PR action"),
        VerifiedSourceWebhook::Ignored
    ));
}

#[test]
fn ignores_non_state_installation_actions_and_rejects_confused_lifecycle_identity() {
    let verifier = verifier(1024 * 1024);
    let created = serde_json::to_vec(&json!({
        "action": "created",
        "installation": {
            "id": 42,
            "account": {"id": 100, "login": "A3S-Lab", "type": "Organization"}
        },
        "sender": {"id": 200, "login": "octocat"}
    }))
    .expect("created payload");
    let created_signature = signature(SECRET, &created);
    assert!(matches!(
        verifier
            .verify(request(
                "installation",
                "delivery-created",
                &created_signature,
                &created,
            ))
            .expect("signed created webhook"),
        VerifiedSourceWebhook::Ignored
    ));

    let confused = serde_json::to_vec(&json!({
        "action": "renamed",
        "installation": {"id": 42},
        "account": {"id": 100, "login": "A3S-Platform", "type": "Organization"},
        "changes": {"login": {"from": "A3S-Lab"}},
        "target_type": "User"
    }))
    .expect("confused payload");
    let confused_signature = signature(SECRET, &confused);
    assert!(matches!(
        verifier.verify(request(
            "installation_target",
            "delivery-confused",
            &confused_signature,
            &confused,
        )),
        Err(SourceWebhookVerificationError::Invalid(_))
    ));
}

#[test]
fn rejects_oversize_or_confused_repository_payloads() {
    let verifier = verifier(1024);
    let oversized = vec![b'x'; 1025];
    assert!(matches!(
        verifier.verify(request("push", "delivery-large", "sha256=00", &oversized)),
        Err(SourceWebhookVerificationError::PayloadTooLarge {
            maximum_bytes: 1024
        })
    ));

    let mut confused: Value = serde_json::from_slice(&payload()).expect("payload JSON");
    confused["repository"]["full_name"] = Value::String("A3S-Lab/Runtime".into());
    let confused = serde_json::to_vec(&confused).expect("confused payload");
    let signature = signature(SECRET, &confused);
    assert!(matches!(
        verifier.verify(request("push", "delivery-2", &signature, &confused)),
        Err(SourceWebhookVerificationError::Invalid(_))
    ));
}

#[test]
fn validates_secret_reference_and_limits_without_exposing_secret_in_debug() {
    assert!(GithubWebhookVerifier::new("not-loud", 1024).is_err());
    assert!(GithubWebhookVerifier::new("A3S_WEBHOOK_SECRET", 1023).is_err());
    assert!(GithubWebhookVerifier::for_test("short", 1024).is_err());
    let verifier = verifier(1024);
    assert!(!format!("{verifier:?}").contains(SECRET));
}

fn verifier(maximum_body_bytes: usize) -> GithubWebhookVerifier {
    GithubWebhookVerifier::for_test(SECRET, maximum_body_bytes).expect("webhook verifier")
}

fn request<'a>(
    event: &'a str,
    delivery_id: &'a str,
    signature: &'a str,
    body: &'a [u8],
) -> SourceWebhookVerificationRequest<'a> {
    SourceWebhookVerificationRequest {
        event,
        delivery_id,
        signature,
        body,
    }
}

fn payload() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "ref": "refs/heads/main",
        "after": "0123456789abcdef0123456789abcdef01234567",
        "deleted": false,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42},
        "ignored": {"providerFields": true}
    }))
    .expect("payload")
}

fn pull_request_payload(
    action: &str,
    state: &str,
    merged: bool,
    head_repository: Option<Value>,
) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "action": action,
        "number": 42,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42},
        "pull_request": {
            "id": 1_000_042,
            "number": 42,
            "state": state,
            "merged": merged,
            "created_at": "2026-08-24T04:30:00Z",
            "updated_at": "2026-08-24T05:30:00.123456789Z",
            "head": {
                "ref": "feature/preview",
                "sha": "fedcba9876543210fedcba9876543210fedcba98",
                "repo": head_repository
            },
            "base": {
                "ref": "main",
                "sha": "0123456789abcdef0123456789abcdef01234567",
                "repo": {
                    "full_name": "A3S-Lab/Cloud",
                    "html_url": "https://github.com/A3S-Lab/Cloud"
                }
            }
        },
        "ignored": {"providerFields": true}
    }))
    .expect("pull-request payload")
}

fn signature(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC");
    mac.update(body);
    format!("sha256={:x}", mac.finalize().into_bytes())
}
