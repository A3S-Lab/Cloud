use super::{
    AdmitAutomationWebhookDelivery, AutomationWebhookAdmissionService,
    AutomationWebhookEndpointQueryService, AutomationWebhookEndpointScope,
    AutomationWebhookReceiver, ChangeAutomationWebhookEndpoint, CreateAutomationWebhookEndpoint,
    EndpointLifecycleAction, ReceiveAutomationWebhookDelivery, ResolveAutomationWebhookEndpoint,
};
use crate::modules::automations::domain::{
    AutomationWebhookEndpointRecord, AutomationWebhookInvocationFactory,
    AutomationWebhookInvocationRequest, IAutomationWebhookAuthorizationSnapshotProvider,
    IAutomationWebhookRepository, IAutomationWebhookSchemaValidator,
    IAutomationWebhookSignatureVerifier,
};
use crate::modules::automations::infrastructure::{
    DigestBoundJsonSchemaValidator, InMemoryAutomationWebhookRepository,
    AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest};
use a3s_cloud_contracts::{
    AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationRevisionV1, AutomationTriggerV1, AutomationWebhookEndpointV1,
    AutomationWebhookRequestV1, AutomationWebhookSecretReferenceV1,
    AutomationWebhookSignatureAlgorithmV1, AutomationWebhookSignatureV1,
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

#[async_trait]
impl IAutomationWebhookAuthorizationSnapshotProvider for AcceptAll {
    async fn resolve(
        &self,
        _endpoint: &AutomationWebhookEndpointV1,
        revision: &AutomationRevisionV1,
    ) -> Result<AutomationInvocationAuthorizationV1, String> {
        Ok(AutomationInvocationAuthorizationV1 {
            policy_digest: revision
                .spec()
                .definition
                .authorization
                .policy_digest
                .clone(),
            grant_snapshot_digest: digest('b'),
            principal_id: None,
        })
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

fn revision_with_schema_digest(schema_digest: &str) -> AutomationRevisionV1 {
    let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
    let mut spec = definition.spec().clone();
    let AutomationTriggerV1::Webhook(trigger) = &mut spec.trigger else {
        panic!("webhook trigger")
    };
    trigger.request_schema_digest = schema_digest.into();
    let definition = AutomationDefinitionV1::from_spec(spec).expect("definition with schema");
    AutomationRevisionV1::from_definition(id(0x110), 1, None, definition.spec().clone())
        .expect("revision")
}

fn schema_digest(schema: &serde_json::Value) -> String {
    sha256_digest(
        &canonical_json_bounded(schema, AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES, "test schema")
            .expect("canonical schema"),
    )
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
    let invocation =
        AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
            endpoint,
            revision,
            request: &request,
            invocation_id: Uuid::from_u128(id(0x200).as_u128() ^ delivery_id.as_u128()),
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
        })
        .expect("webhook invocation");
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
    let query_service = AutomationWebhookEndpointQueryService::new(repository.clone());
    let found_by_key = query_service
        .resolve(ResolveAutomationWebhookEndpoint {
            scope: AutomationWebhookEndpointScope {
                organization_id: created.endpoint.organization_id,
                project_id: created.endpoint.project_id,
                environment_id: created.endpoint.environment_id,
            },
            endpoint_key: created.endpoint.endpoint_key.clone(),
        })
        .await
        .expect("find endpoint by key")
        .expect("endpoint by key");
    assert_eq!(found_by_key, created);
    assert!(query_service
        .resolve(ResolveAutomationWebhookEndpoint {
            scope: AutomationWebhookEndpointScope {
                organization_id: created.endpoint.organization_id,
                project_id: created.endpoint.project_id,
                environment_id: Uuid::from_u128(created.endpoint.environment_id.as_u128() ^ 1,),
            },
            endpoint_key: created.endpoint.endpoint_key.clone(),
        })
        .await
        .expect("scoped lookup")
        .is_none());

    assert!(matches!(
        query_service
            .resolve(ResolveAutomationWebhookEndpoint {
                scope: AutomationWebhookEndpointScope {
                    organization_id: Uuid::nil(),
                    project_id: created.endpoint.project_id,
                    environment_id: created.endpoint.environment_id,
                },
                endpoint_key: created.endpoint.endpoint_key.clone(),
            })
            .await,
        Err(ApplicationError::Invalid(message)) if message.contains("must not be nil")
    ));
    assert!(matches!(
        query_service
            .resolve(ResolveAutomationWebhookEndpoint {
                scope: AutomationWebhookEndpointScope {
                    organization_id: created.endpoint.organization_id,
                    project_id: created.endpoint.project_id,
                    environment_id: created.endpoint.environment_id,
                },
                endpoint_key: "invalid/key".into(),
            })
            .await,
        Err(ApplicationError::Invalid(message)) if message.contains("bounded opaque key")
    ));

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
async fn webhook_receiver_composes_scoped_capture_and_lifecycle_admission() {
    let repository = Arc::new(InMemoryAutomationWebhookRepository::new());
    let admission = Arc::new(service(repository.clone()));
    let command = create_command(revision());
    let created = admission
        .create_endpoint(command.clone())
        .await
        .expect("create endpoint");
    let endpoint = created.endpoint.clone();
    let receiver = AutomationWebhookReceiver::new(
        Arc::new(AutomationWebhookEndpointQueryService::new(
            repository.clone(),
        )),
        admission.clone(),
        Arc::new(AcceptAll),
    );
    let scope = AutomationWebhookEndpointScope {
        organization_id: endpoint.organization_id,
        project_id: endpoint.project_id,
        environment_id: endpoint.environment_id,
    };
    let received_at = timestamp("2026-09-05T00:00:03.000Z");
    let accepted = receiver
        .receive(ReceiveAutomationWebhookDelivery {
            scope,
            endpoint_key: endpoint.endpoint_key.clone(),
            delivery_id: id(0x601),
            signature: AutomationWebhookSignatureV1 {
                algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
                key_version: endpoint.signing_secret.version,
                value: format!("hmac-sha256:{}", "a".repeat(64)),
            },
            content_type: "application/json".into(),
            body: br#"{"release":"stable"}"#.to_vec(),
            received_at,
            invocation_id: id(0x602),
            requested_at: received_at,
            correlation_id: id(0x603),
            causation_id: None,
            receipt_id: id(0x604),
            recorded_at: received_at,
        })
        .await
        .expect("accepted webhook");
    assert!(!accepted.replayed);
    assert_eq!(
        accepted.delivery.receipt.decision,
        a3s_cloud_contracts::AutomationWebhookAdmissionDecisionV1::Admitted
    );
    assert!(accepted.delivery.invocation.is_some());

    admission
        .change_endpoint(ChangeAutomationWebhookEndpoint {
            endpoint_id: endpoint.endpoint_id,
            expected_generation: 1,
            action: EndpointLifecycleAction::Disable,
            changed_at: received_at + chrono::Duration::seconds(1),
        })
        .await
        .expect("disable endpoint");
    let rejected = receiver
        .receive(ReceiveAutomationWebhookDelivery {
            scope,
            endpoint_key: endpoint.endpoint_key,
            delivery_id: id(0x605),
            signature: AutomationWebhookSignatureV1 {
                algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
                key_version: endpoint.signing_secret.version,
                value: format!("hmac-sha256:{}", "a".repeat(64)),
            },
            content_type: "application/json".into(),
            body: br#"{"release":"disabled"}"#.to_vec(),
            received_at: received_at + chrono::Duration::seconds(2),
            invocation_id: id(0x606),
            requested_at: received_at,
            correlation_id: id(0x607),
            causation_id: None,
            receipt_id: id(0x608),
            recorded_at: received_at + chrono::Duration::seconds(2),
        })
        .await
        .expect("disabled lifecycle receipt");
    assert_eq!(
        rejected.delivery.receipt.decision,
        a3s_cloud_contracts::AutomationWebhookAdmissionDecisionV1::Rejected
    );
    assert_eq!(
        rejected.delivery.receipt.rejection_reason,
        Some(a3s_cloud_contracts::AutomationWebhookRejectionReasonV1::EndpointDisabled)
    );
    assert!(rejected.delivery.invocation.is_none());
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

#[tokio::test]
async fn active_admission_rejects_missing_invocation_before_external_validation() {
    let repository = Arc::new(InMemoryAutomationWebhookRepository::new());
    let command = create_command(revision());
    let endpoint = endpoint_from(&command);
    let created = service(repository.clone())
        .create_endpoint(command)
        .await
        .expect("create endpoint");
    let (request, _) = request_and_invocation(
        &endpoint,
        &created.revision,
        id(0x621),
        br#"{"release":"stable"}"#,
        timestamp("2026-09-05T00:00:01.000Z"),
    );
    let rejecting = AutomationWebhookAdmissionService::new(
        repository.clone(),
        Arc::new(RejectingPort),
        Arc::new(RejectingPort),
    );

    let result = rejecting
        .admit(AdmitAutomationWebhookDelivery {
            request,
            invocation: None,
            receipt_id: id(0x622),
            recorded_at: timestamp("2026-09-05T00:00:02.000Z"),
        })
        .await;

    assert!(matches!(
        result,
        Err(ApplicationError::Invalid(message))
            if message == "an active Automation webhook delivery requires an invocation envelope"
    ));
    assert!(repository
        .find_delivery(endpoint.endpoint_id, id(0x621))
        .await
        .expect("delivery lookup")
        .is_none());
    assert!(repository.receipts().await.is_empty());
    assert!(repository.audit_records().await.is_empty());
    assert!(repository.outbox_messages().await.is_empty());
}

#[tokio::test]
async fn digest_bound_schema_rejection_prevents_delivery_persistence() {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["release"],
        "properties": {"release": {"type": "string"}},
        "additionalProperties": false
    });
    let schema_digest = schema_digest(&schema);
    let schema_validator = DigestBoundJsonSchemaValidator::new(schema_digest.clone(), schema)
        .expect("digest-bound schema validator");
    let repository = Arc::new(InMemoryAutomationWebhookRepository::new());
    let command = create_command(revision_with_schema_digest(&schema_digest));
    let endpoint = endpoint_from(&command);
    let service = AutomationWebhookAdmissionService::new(
        repository.clone(),
        Arc::new(AcceptAll),
        Arc::new(schema_validator),
    );
    let created = service
        .create_endpoint(command)
        .await
        .expect("create endpoint");
    let delivery_id = id(0x611);
    let (request, invocation) = request_and_invocation(
        &endpoint,
        &created.revision,
        delivery_id,
        br#"{"release":7}"#,
        timestamp("2026-09-05T00:00:01.000Z"),
    );

    let result = service
        .admit(AdmitAutomationWebhookDelivery {
            request,
            invocation: Some(invocation),
            receipt_id: id(0x612),
            recorded_at: timestamp("2026-09-05T00:00:02.000Z"),
        })
        .await;
    assert!(matches!(
        result,
        Err(ApplicationError::Invalid(message))
            if message == "Automation webhook payload does not satisfy its request schema"
    ));
    assert!(repository
        .find_delivery(endpoint.endpoint_id, delivery_id)
        .await
        .expect("delivery lookup")
        .is_none());
    assert!(repository.receipts().await.is_empty());
    assert!(repository.audit_records().await.is_empty());
    assert!(repository.outbox_messages().await.is_empty());
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
