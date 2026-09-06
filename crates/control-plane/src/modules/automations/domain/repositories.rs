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

/// One exact invocation envelope admitted by Automations.
///
/// The canonical digest is retained beside the envelope so a replay cannot
/// replace an invocation with different immutable evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationInvocationRecord {
    pub envelope: AutomationInvocationEnvelopeV1,
    pub digest: String,
}

impl AutomationInvocationRecord {
    pub fn new(envelope: AutomationInvocationEnvelopeV1) -> Result<Self, String> {
        envelope.validate()?;
        let digest = envelope.digest()?;
        Ok(Self { envelope, digest })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationInvocationAdmission {
    pub invocation: AutomationInvocationRecord,
    pub replayed: bool,
}

#[async_trait]
pub trait IAutomationInvocationRepository: Send + Sync {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> Result<AutomationInvocationAdmission, RepositoryError>;
}

/// Read-only recovery port for consumers of the committed invocation Outbox.
/// The lookup is scoped by Organization and exact invocation identity so a
/// published digest can be checked against the durable envelope before any
/// target owner receives it.
#[async_trait]
pub trait IAutomationInvocationReader: Send + Sync {
    async fn find(
        &self,
        organization_id: Uuid,
        invocation_id: Uuid,
    ) -> Result<Option<AutomationInvocationRecord>, RepositoryError>;
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

    /// Resolve one opaque endpoint key within its complete tenant scope.
    ///
    /// Public transport may know the scope and route key without knowing the
    /// internal endpoint UUID.  Implementations must not widen this lookup to
    /// organization-only or global key matching.
    async fn find_endpoint_by_key(
        &self,
        organization_id: Uuid,
        project_id: Uuid,
        environment_id: Uuid,
        endpoint_key: &str,
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
