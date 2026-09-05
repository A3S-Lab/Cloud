use super::{
    AdmitAutomationWebhookDelivery, AutomationWebhookAdmissionService,
    ChangeAutomationWebhookEndpoint, CreateAutomationWebhookEndpoint, EndpointLifecycleAction,
};
use crate::modules::automations::domain::{
    AutomationWebhookEndpointRecord, IAutomationWebhookRepository,
    IAutomationWebhookSchemaValidator, IAutomationWebhookSignatureVerifier,
};
use crate::modules::automations::infrastructure::InMemoryAutomationWebhookRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_contracts::{
    AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationInvocationInputV1, AutomationInvocationOriginV1, AutomationRevisionV1,
    AutomationWebhookEndpointV1, AutomationWebhookRequestV1, AutomationWebhookSecretReferenceV1,
    AutomationWebhookSignatureAlgorithmV1, AutomationWebhookSignatureV1,
    AUTOMATION_INVOCATION_SCHEMA_V1,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

const WEBHOOK_DEFINITION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/aut0.1/automation-definition-webhook.acl"
));

#[derive(Default)]
struct AcceptAll;

#[async_trait]
impl IAutomationWebhookSignatureVerifier for AcceptAll {
    async fn verify(
        &self,
        _endpoint: &AutomationWebhookEndpointV1,
        _request: &AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
impl IAutomationWebhookSchemaValidator for AcceptAll {
    async fn validate(
        &self,
        _endpoint: &AutomationWebhookEndpointV1,
        _request: &AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        Ok(())
    }
}

struct RejectingPort;

#[async_trait]
impl IAutomationWebhookSignatureVerifier for RejectingPort {
    async fn verify(
        &self,
        _endpoint: &AutomationWebhookEndpointV1,
        _request: &AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        Err("signature verifier rejected the normalized fact".into())
    }
}

#[async_trait]
impl IAutomationWebhookSchemaValidator for RejectingPort {
    async fn validate(
        &self,
        _endpoint: &AutomationWebhookEndpointV1,
        _request: &AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        Err("schema evaluator rejected the payload".into())
    }
}

fn service(
    repository: Arc<InMemoryAutomationWebhookRepository>,
) -> AutomationWebhookAdmissionService {
    AutomationWebhookAdmissionService::new(repository, Arc::new(AcceptAll), Arc::new(AcceptAll))
}

fn revision() -> AutomationRevisionV1 {
    let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
    AutomationRevisionV1::from_definition(id(0x100), 1, None, definition.spec().clone())
        .expect("revision")
}

fn create_command(revision: AutomationRevisionV1) -> CreateAutomationWebhookEndpoint {
    CreateAutomationWebhookEndpoint {
        endpoint_id: id(0x101),
        endpoint_key: "release-hook".into(),
        signing_secret: AutomationWebhookSecretReferenceV1 {
            secret_id: id(0x102),
            version: 4,
        },
        max_body_bytes: 4_096,
        revision,
        created_at: timestamp("2026-09-05T00:00:00.000Z"),
    }
}

fn endpoint_from(command: &CreateAutomationWebhookEndpoint) -> AutomationWebhookEndpointV1 {
    AutomationWebhookEndpointV1::for_revision(
        command.endpoint_id,
        command.endpoint_key.clone(),
        command.signing_secret,
        command.max_body_bytes,
        &command.revision,
        command.created_at,
    )
    .expect("endpoint")
}

fn request_and_invocation(
    endpoint: &AutomationWebhookEndpointV1,
    revision: &AutomationRevisionV1,
    delivery_id: Uuid,
    body: &[u8],
    received_at: DateTime<Utc>,
) -> (AutomationWebhookRequestV1, AutomationInvocationEnvelopeV1) {
    let request = AutomationWebhookRequestV1::from_json(
        endpoint,
        delivery_id,
        AutomationWebhookSignatureV1 {
            algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
            key_version: endpoint.signing_secret.version,
            value: format!("hmac-sha256:{}", "a".repeat(64)),
        },
        "application/json",
        body,
        received_at,
    )
    .expect("request");
    let origin = AutomationInvocationOriginV1::Event {
        event_id: delivery_id,
        event_key: "automation.webhook.received".into(),
        event_digest: request.body_digest.clone(),
        observed_at: received_at,
    };
    let subscription = revision
        .spec()
        .definition
        .trigger
        .subscription()
        .expect("webhook subscription");
    let deduplication_key = revision
        .spec()
        .definition
        .policy
        .deduplication
        .render_key(
            revision.spec().definition.automation_id,
            revision.spec().revision_id,
            &origin,
            Some(subscription.subscription_id),
        )
        .expect("deduplication key");
    let invocation = AutomationInvocationEnvelopeV1 {
        schema: AUTOMATION_INVOCATION_SCHEMA_V1.into(),
        invocation_id: Uuid::from_u128(id(0x200).as_u128() ^ delivery_id.as_u128()),
        automation_id: revision.spec().definition.automation_id,
        automation_revision_id: revision.spec().revision_id,
        automation_revision_digest: revision.digest().into(),
        organization_id: revision.spec().definition.organization_id,
        project_id: revision.spec().definition.project_id,
        environment_id: revision.spec().definition.environment_id,
        target: revision.spec().definition.target.clone(),
        origin,
        subscription: Some(subscription.clone()),
        deduplication_key,
        input: AutomationInvocationInputV1::inline_json(
            serde_json::from_slice(body).expect("JSON body"),
        )
        .expect("input"),
        authorization: AutomationInvocationAuthorizationV1 {
            policy_digest: revision
                .spec()
                .definition
                .authorization
                .policy_digest
                .clone(),
            grant_snapshot_digest: digest('b'),
            principal_id: None,
        },
        requested_at: received_at,
        correlation_id: Uuid::from_u128(id(0x300).as_u128() ^ delivery_id.as_u128()),
        causation_id: None,
    };
    (request, invocation)
}

#[tokio::test]
async fn endpoint_registration_pins_revision_and_rejects_scope_key_collisions() {
    let repository = Arc::new(InMemoryAutomationWebhookRepository::new());
    let service = service(repository.clone());
    let command = create_command(revision());
    let created = service
        .create_endpoint(command.clone())
        .await
        .expect("create endpoint");
    assert_eq!(created.endpoint.generation, 1);
    assert_eq!(created.endpoint.revision_digest, created.revision.digest());
    assert_eq!(created.endpoint.endpoint_key, "release-hook");

    let found = repository
        .find_endpoint(command.endpoint_id)
        .await
        .expect("find endpoint")
        .expect("endpoint");
    assert_eq!(found, created);

    let duplicate = CreateAutomationWebhookEndpoint {
        endpoint_id: id(0x104),
        ..command
    };
    assert!(matches!(
        service.create_endpoint(duplicate).await,
        Err(ApplicationError::Conflict(message)) if message.contains("key")
    ));
}

#[tokio::test]
async fn admission_replays_once_and_conflicts_on_delivery_body_drift() {
    let repository = Arc::new(InMemoryAutomationWebhookRepository::new());
    let service = service(repository.clone());
    let command = create_command(revision());
    let endpoint = endpoint_from(&command);
    let created = service
        .create_endpoint(command)
        .await
        .expect("create endpoint");
    let delivery_id = id(0x401);
    let received_at = timestamp("2026-09-05T00:00:01.000Z");
    let (request, invocation) = request_and_invocation(
        &endpoint,
        &created.revision,
        delivery_id,
        br#"{"release":"stable"}"#,
        received_at,
    );
    let first = service
        .admit(AdmitAutomationWebhookDelivery {
            request: request.clone(),
            invocation: Some(invocation.clone()),
            receipt_id: id(0x402),
            recorded_at: timestamp("2026-09-05T00:00:02.000Z"),
        })
        .await
        .expect("first admission");
    assert!(!first.replayed);
    assert_eq!(
        first.delivery.receipt.decision,
        a3s_cloud_contracts::AutomationWebhookAdmissionDecisionV1::Admitted
    );

    let replay = service
        .admit(AdmitAutomationWebhookDelivery {
            request: request.clone(),
            invocation: Some(invocation.clone()),
            receipt_id: id(0x403),
            recorded_at: timestamp("2026-09-05T00:00:03.000Z"),
        })
        .await
        .expect("replay admission");
    assert!(replay.replayed);
    assert_eq!(
        replay.delivery.receipt.decision,
        a3s_cloud_contracts::AutomationWebhookAdmissionDecisionV1::Replayed
    );
    assert_eq!(replay.delivery.invocation, Some(invocation));

    let (changed_request, changed_invocation) = request_and_invocation(
        &endpoint,
        &created.revision,
        delivery_id,
        br#"{"release":"canary"}"#,
        received_at,
    );
    let conflict = service
        .admit(AdmitAutomationWebhookDelivery {
            request: changed_request,
            invocation: Some(changed_invocation),
            receipt_id: id(0x404),
            recorded_at: timestamp("2026-09-05T00:00:04.000Z"),
        })
        .await
        .expect("duplicate conflict receipt");
    assert!(!conflict.replayed);
    assert_eq!(
        conflict.delivery.receipt.rejection_reason,
        Some(a3s_cloud_contracts::AutomationWebhookRejectionReasonV1::DuplicateDeliveryConflict)
    );
    assert_eq!(repository.outbox_messages().await.len(), 1);
    assert_eq!(repository.audit_records().await.len(), 2);
    assert_eq!(repository.receipts().await.len(), 3);
}

#[tokio::test]
async fn lifecycle_generation_is_compare_and_swap_and_rejections_are_durable() {
    let repository = Arc::new(InMemoryAutomationWebhookRepository::new());
    let service = service(repository.clone());
    let command = create_command(revision());
    let endpoint = endpoint_from(&command);
    let created = service
        .create_endpoint(command)
        .await
        .expect("create endpoint");
    let disabled = service
        .change_endpoint(ChangeAutomationWebhookEndpoint {
            endpoint_id: created.endpoint.endpoint_id,
            expected_generation: 1,
            action: EndpointLifecycleAction::Disable,
            changed_at: timestamp("2026-09-05T00:00:01.000Z"),
        })
        .await
        .expect("disable endpoint");
    assert_eq!(disabled.endpoint.generation, 2);
    assert_eq!(
        disabled.endpoint.state,
        a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Disabled
    );

    let stale = service
        .change_endpoint(ChangeAutomationWebhookEndpoint {
            endpoint_id: created.endpoint.endpoint_id,
            expected_generation: 1,
            action: EndpointLifecycleAction::Enable,
            changed_at: timestamp("2026-09-05T00:00:02.000Z"),
        })
        .await;
    assert!(matches!(stale, Err(ApplicationError::Conflict(message)) if message.contains("stale")));

    let (request, _) = request_and_invocation(
        &endpoint,
        &created.revision,
        id(0x501),
        br#"{"release":"stable"}"#,
        timestamp("2026-09-05T00:00:03.000Z"),
    );
    let rejected = service
        .admit(AdmitAutomationWebhookDelivery {
            request,
            invocation: None,
            receipt_id: id(0x502),
            recorded_at: timestamp("2026-09-05T00:00:04.000Z"),
        })
        .await
        .expect("disabled rejection");
    assert_eq!(
        rejected.delivery.receipt.rejection_reason,
        Some(a3s_cloud_contracts::AutomationWebhookRejectionReasonV1::EndpointDisabled)
    );

    let enabled = service
        .change_endpoint(ChangeAutomationWebhookEndpoint {
            endpoint_id: created.endpoint.endpoint_id,
            expected_generation: 2,
            action: EndpointLifecycleAction::Enable,
            changed_at: timestamp("2026-09-05T00:00:05.000Z"),
        })
        .await
        .expect("enable endpoint");
    assert_eq!(enabled.endpoint.generation, 3);
    let revoked = service
        .change_endpoint(ChangeAutomationWebhookEndpoint {
            endpoint_id: created.endpoint.endpoint_id,
            expected_generation: 3,
            action: EndpointLifecycleAction::Revoke,
            changed_at: timestamp("2026-09-05T00:00:06.000Z"),
        })
        .await
        .expect("revoke endpoint");
    assert_eq!(revoked.endpoint.generation, 4);

    let stored = repository
        .find_delivery(created.endpoint.endpoint_id, id(0x501))
        .await
        .expect("find rejected delivery")
        .expect("rejected delivery");
    assert_eq!(stored.invocation, None);
}

#[tokio::test]
async fn verifier_and_schema_ports_are_required_before_persistence() {
    let repository = Arc::new(InMemoryAutomationWebhookRepository::new());
    let command = create_command(revision());
    let endpoint = endpoint_from(&command);
    let created = service(repository.clone())
        .create_endpoint(command)
        .await
        .expect("create endpoint");
    let (request, invocation) = request_and_invocation(
        &endpoint,
        &created.revision,
        id(0x601),
        br#"{"release":"stable"}"#,
        timestamp("2026-09-05T00:00:01.000Z"),
    );
    let rejecting = AutomationWebhookAdmissionService::new(
        repository.clone(),
        Arc::new(RejectingPort),
        Arc::new(AcceptAll),
    );
    assert!(matches!(
        rejecting
            .admit(AdmitAutomationWebhookDelivery {
                request: request.clone(),
                invocation: Some(invocation.clone()),
                receipt_id: id(0x602),
                recorded_at: timestamp("2026-09-05T00:00:02.000Z"),
            })
            .await,
        Err(ApplicationError::Invalid(message)) if message.contains("signature")
    ));
    assert!(repository
        .find_delivery(endpoint.endpoint_id, id(0x601))
        .await
        .expect("delivery lookup")
        .is_none());

    let schema_rejecting = AutomationWebhookAdmissionService::new(
        repository.clone(),
        Arc::new(AcceptAll),
        Arc::new(RejectingPort),
    );
    assert!(matches!(
        schema_rejecting
            .admit(AdmitAutomationWebhookDelivery {
                request,
                invocation: Some(invocation),
                receipt_id: id(0x603),
                recorded_at: timestamp("2026-09-05T00:00:02.000Z"),
            })
            .await,
        Err(ApplicationError::Invalid(message)) if message.contains("schema")
    ));
}

#[test]
fn endpoint_record_contains_secret_reference_but_never_secret_material() {
    let revision = revision();
    let command = create_command(revision.clone());
    let endpoint = endpoint_from(&command);
    let record = AutomationWebhookEndpointRecord::new(endpoint, revision).expect("record");
    let encoded = serde_json::to_string(&record.endpoint).expect("endpoint JSON");
    assert!(encoded.contains(&command.signing_secret.secret_id.to_string()));
    assert!(encoded.contains(&command.signing_secret.version.to_string()));
    assert!(!encoded.contains("plaintext"));
    assert!(!encoded.contains("secretMaterial"));
}

fn id(value: u16) -> Uuid {
    Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0000_u128 + u128::from(value))
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp")
        .with_timezone(&Utc)
}

#[allow(dead_code)]
fn _json_fixture() -> serde_json::Value {
    json!({"release": "stable"})
}
