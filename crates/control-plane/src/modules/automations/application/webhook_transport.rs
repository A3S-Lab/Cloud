use super::endpoint_query::{
    AutomationWebhookEndpointQueryService, AutomationWebhookEndpointScope,
    ResolveAutomationWebhookEndpoint,
};
use super::webhook_admission::{AdmitAutomationWebhookDelivery, AutomationWebhookAdmissionService};
use crate::modules::automations::domain::{
    AutomationWebhookAdmission, IAutomationWebhookAuthorizationSnapshotProvider,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_cloud_contracts::{AutomationWebhookRequestV1, AutomationWebhookSignatureV1};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

/// Captured input supplied by an HTTP or Gateway adapter.
///
/// The adapter is responsible only for extracting the opaque route key and
/// transport headers. Authorization is resolved by the injected owner port;
/// this boundary never accepts caller-supplied grants or principals.
#[derive(Debug, Clone)]
pub struct ReceiveAutomationWebhookDelivery {
    pub scope: AutomationWebhookEndpointScope,
    pub endpoint_key: String,
    pub delivery_id: Uuid,
    pub signature: AutomationWebhookSignatureV1,
    pub content_type: String,
    pub body: Vec<u8>,
    pub received_at: DateTime<Utc>,
    pub invocation_id: Uuid,
    pub requested_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub receipt_id: Uuid,
    pub recorded_at: DateTime<Utc>,
}

/// Application-owned webhook receive composition.
///
/// This is deliberately independent of an HTTP framework. It resolves the
/// endpoint within the complete scope, captures the bounded canonical request,
/// constructs an exact invocation only for an accepting endpoint, and enters
/// the single durable admission service. Public route registration and
/// Gateway recovery remain presentation/owner composition concerns.
#[derive(Clone)]
pub struct AutomationWebhookReceiver {
    endpoint_query: Arc<AutomationWebhookEndpointQueryService>,
    admission: Arc<AutomationWebhookAdmissionService>,
    authorization: Arc<dyn IAutomationWebhookAuthorizationSnapshotProvider>,
}

impl AutomationWebhookReceiver {
    pub fn new(
        endpoint_query: Arc<AutomationWebhookEndpointQueryService>,
        admission: Arc<AutomationWebhookAdmissionService>,
        authorization: Arc<dyn IAutomationWebhookAuthorizationSnapshotProvider>,
    ) -> Self {
        Self {
            endpoint_query,
            admission,
            authorization,
        }
    }

    pub async fn receive(
        &self,
        command: ReceiveAutomationWebhookDelivery,
    ) -> ApplicationResult<AutomationWebhookAdmission> {
        let record = self
            .endpoint_query
            .resolve(ResolveAutomationWebhookEndpoint {
                scope: command.scope,
                endpoint_key: command.endpoint_key,
            })
            .await?
            .ok_or_else(|| ApplicationError::NotFound("webhook endpoint not found".into()))?;

        let request = AutomationWebhookRequestV1::from_json(
            &record.endpoint,
            command.delivery_id,
            command.signature,
            command.content_type,
            &command.body,
            command.received_at,
        )
        .map_err(ApplicationError::Invalid)?;

        // Inactive endpoints still receive an immutable lifecycle rejection
        // receipt. Do not construct or validate an invocation first: doing so
        // could turn a valid disable/revoke receipt into an authorization or
        // timestamp error before the admission authority records it.
        let invocation = if record.endpoint.state.is_accepting() {
            let authorization = self
                .authorization
                .resolve(&record.endpoint, &record.revision)
                .await
                .map_err(|_| {
                    ApplicationError::Forbidden(
                        "Automation webhook authorization snapshot is unavailable".into(),
                    )
                })?;
            Some(
                crate::modules::automations::domain::AutomationWebhookInvocationFactory::build(
                    crate::modules::automations::domain::AutomationWebhookInvocationRequest {
                        endpoint: &record.endpoint,
                        revision: &record.revision,
                        request: &request,
                        invocation_id: command.invocation_id,
                        requested_at: command.requested_at,
                        authorization,
                        correlation_id: command.correlation_id,
                        causation_id: command.causation_id,
                    },
                )
                .map_err(ApplicationError::Invalid)?,
            )
        } else {
            None
        };

        self.admission
            .admit(AdmitAutomationWebhookDelivery {
                request,
                invocation,
                receipt_id: command.receipt_id,
                recorded_at: command.recorded_at,
            })
            .await
    }
}
