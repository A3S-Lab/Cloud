use crate::modules::automations::domain::{
    AdmitAutomationWebhookDeliveryWrite, AutomationWebhookAdmission,
    AutomationWebhookEndpointRecord, EndpointLifecycleAction, IAutomationWebhookRepository,
    IAutomationWebhookSchemaValidator, IAutomationWebhookSignatureVerifier,
    TransitionAutomationWebhookEndpoint,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_cloud_contracts::{
    AutomationRevisionV1, AutomationWebhookEndpointV1, AutomationWebhookRequestV1,
    AutomationWebhookSecretReferenceV1,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateAutomationWebhookEndpoint {
    pub endpoint_id: Uuid,
    pub endpoint_key: String,
    pub signing_secret: AutomationWebhookSecretReferenceV1,
    pub max_body_bytes: u64,
    pub revision: AutomationRevisionV1,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AdmitAutomationWebhookDelivery {
    pub request: AutomationWebhookRequestV1,
    pub invocation: Option<a3s_cloud_contracts::AutomationInvocationEnvelopeV1>,
    pub receipt_id: Uuid,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ChangeAutomationWebhookEndpoint {
    pub endpoint_id: Uuid,
    pub expected_generation: u64,
    pub action: EndpointLifecycleAction,
    pub changed_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AutomationWebhookAdmissionService {
    repository: Arc<dyn IAutomationWebhookRepository>,
    signature_verifier: Arc<dyn IAutomationWebhookSignatureVerifier>,
    schema_validator: Arc<dyn IAutomationWebhookSchemaValidator>,
}

impl AutomationWebhookAdmissionService {
    pub fn new(
        repository: Arc<dyn IAutomationWebhookRepository>,
        signature_verifier: Arc<dyn IAutomationWebhookSignatureVerifier>,
        schema_validator: Arc<dyn IAutomationWebhookSchemaValidator>,
    ) -> Self {
        Self {
            repository,
            signature_verifier,
            schema_validator,
        }
    }

    pub async fn create_endpoint(
        &self,
        command: CreateAutomationWebhookEndpoint,
    ) -> ApplicationResult<AutomationWebhookEndpointRecord> {
        let endpoint = AutomationWebhookEndpointV1::for_revision(
            command.endpoint_id,
            command.endpoint_key,
            command.signing_secret,
            command.max_body_bytes,
            &command.revision,
            command.created_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let record = AutomationWebhookEndpointRecord::new(endpoint, command.revision)
            .map_err(ApplicationError::Invalid)?;
        self.repository
            .create_endpoint(record)
            .await
            .map_err(Into::into)
    }

    pub async fn change_endpoint(
        &self,
        command: ChangeAutomationWebhookEndpoint,
    ) -> ApplicationResult<AutomationWebhookEndpointRecord> {
        self.repository
            .transition_endpoint(TransitionAutomationWebhookEndpoint {
                endpoint_id: command.endpoint_id,
                expected_generation: command.expected_generation,
                action: command.action,
                changed_at: command.changed_at,
            })
            .await
            .map_err(Into::into)
    }

    pub async fn admit(
        &self,
        command: AdmitAutomationWebhookDelivery,
    ) -> ApplicationResult<AutomationWebhookAdmission> {
        let endpoint_id = command.request.endpoint_id;
        let record = self
            .repository
            .find_endpoint(endpoint_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("webhook endpoint not found".into()))?;

        command
            .request
            .validate_for_endpoint(&record.endpoint)
            .map_err(ApplicationError::Invalid)?;

        // A lifecycle rejection is a durable receipt, not an opportunity to
        // invoke a target.  Signature/schema ports are only consulted for an
        // endpoint that is currently accepting a new delivery.
        if !record.endpoint.state.is_accepting() {
            return self
                .repository
                .admit_delivery(AdmitAutomationWebhookDeliveryWrite {
                    request: command.request,
                    invocation: None,
                    receipt_id: command.receipt_id,
                    recorded_at: command.recorded_at,
                })
                .await
                .map_err(Into::into);
        }

        self.signature_verifier
            .verify(&record.endpoint, &command.request)
            .await
            .map_err(ApplicationError::Invalid)?;
        self.schema_validator
            .validate(&record.endpoint, &command.request)
            .await
            .map_err(ApplicationError::Invalid)?;

        self.repository
            .admit_delivery(AdmitAutomationWebhookDeliveryWrite {
                request: command.request,
                invocation: command.invocation,
                receipt_id: command.receipt_id,
                recorded_at: command.recorded_at,
            })
            .await
            .map_err(Into::into)
    }
}
