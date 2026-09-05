use super::{AutomationWebhookDeliveryRecord, AutomationWebhookEndpointRecord};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{AutomationInvocationEnvelopeV1, AutomationWebhookRequestV1};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointLifecycleAction {
    Disable,
    Enable,
    Revoke,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionAutomationWebhookEndpoint {
    pub endpoint_id: Uuid,
    pub expected_generation: u64,
    pub action: EndpointLifecycleAction,
    pub changed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AdmitAutomationWebhookDeliveryWrite {
    pub request: AutomationWebhookRequestV1,
    pub invocation: Option<AutomationInvocationEnvelopeV1>,
    pub receipt_id: Uuid,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationWebhookAdmission {
    pub delivery: AutomationWebhookDeliveryRecord,
    pub replayed: bool,
}

#[async_trait]
pub trait IAutomationWebhookRepository: Send + Sync {
    async fn create_endpoint(
        &self,
        record: AutomationWebhookEndpointRecord,
    ) -> Result<AutomationWebhookEndpointRecord, RepositoryError>;

    async fn find_endpoint(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<AutomationWebhookEndpointRecord>, RepositoryError>;

    async fn transition_endpoint(
        &self,
        transition: TransitionAutomationWebhookEndpoint,
    ) -> Result<AutomationWebhookEndpointRecord, RepositoryError>;

    async fn find_delivery(
        &self,
        endpoint_id: Uuid,
        delivery_id: Uuid,
    ) -> Result<Option<AutomationWebhookDeliveryRecord>, RepositoryError>;

    async fn admit_delivery(
        &self,
        write: AdmitAutomationWebhookDeliveryWrite,
    ) -> Result<AutomationWebhookAdmission, RepositoryError>;
}
