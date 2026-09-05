use crate::modules::automations::domain::{
    AdmitAutomationWebhookDeliveryWrite, AutomationWebhookAdmission,
    AutomationWebhookDeliveryRecord, AutomationWebhookEndpointRecord, EndpointLifecycleAction,
    IAutomationWebhookRepository, TransitionAutomationWebhookEndpoint,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{
    AutomationAuditActionV1, AutomationAuditRecordV1, AutomationOutboxMessageV1,
    AutomationWebhookAdmissionDecisionV1, AutomationWebhookDeliveryReceiptV1,
    AutomationWebhookRejectionReasonV1,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub struct InMemoryAutomationWebhookRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    endpoints: BTreeMap<Uuid, AutomationWebhookEndpointRecord>,
    endpoint_keys: BTreeMap<(Uuid, Uuid, Uuid, String), Uuid>,
    deliveries: BTreeMap<(Uuid, Uuid), AutomationWebhookDeliveryRecord>,
    receipts: BTreeMap<Uuid, AutomationWebhookDeliveryReceiptV1>,
    audit: Vec<AutomationAuditRecordV1>,
    outbox: Vec<AutomationOutboxMessageV1>,
}

impl InMemoryAutomationWebhookRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn audit_records(&self) -> Vec<AutomationAuditRecordV1> {
        self.state.read().await.audit.clone()
    }

    pub async fn outbox_messages(&self) -> Vec<AutomationOutboxMessageV1> {
        self.state.read().await.outbox.clone()
    }

    pub async fn receipts(&self) -> Vec<AutomationWebhookDeliveryReceiptV1> {
        self.state.read().await.receipts.values().cloned().collect()
    }
}

#[async_trait]
impl IAutomationWebhookRepository for InMemoryAutomationWebhookRepository {
    async fn create_endpoint(
        &self,
        record: AutomationWebhookEndpointRecord,
    ) -> Result<AutomationWebhookEndpointRecord, RepositoryError> {
        record.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let endpoint_id = record.endpoint_id();
        if state.endpoints.contains_key(&endpoint_id) {
            return Err(RepositoryError::Conflict(
                "Automation webhook endpoint identity is already in use".into(),
            ));
        }
        let scope_key = record.scope_key();
        if state.endpoint_keys.contains_key(&scope_key) {
            return Err(RepositoryError::Conflict(
                "Automation webhook endpoint key is already in use in this environment".into(),
            ));
        }
        state.endpoint_keys.insert(scope_key, endpoint_id);
        state.endpoints.insert(endpoint_id, record.clone());
        Ok(record)
    }

    async fn find_endpoint(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<AutomationWebhookEndpointRecord>, RepositoryError> {
        Ok(self.state.read().await.endpoints.get(&endpoint_id).cloned())
    }

    async fn transition_endpoint(
        &self,
        transition: TransitionAutomationWebhookEndpoint,
    ) -> Result<AutomationWebhookEndpointRecord, RepositoryError> {
        let mut state = self.state.write().await;
        let current = state
            .endpoints
            .get(&transition.endpoint_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if current.endpoint.generation != transition.expected_generation {
            return Err(RepositoryError::Conflict(
                "Automation webhook endpoint generation is stale".into(),
            ));
        }
        let mut endpoint = current.endpoint.clone();
        let result = match transition.action {
            EndpointLifecycleAction::Disable => endpoint.disable(transition.changed_at),
            EndpointLifecycleAction::Enable => endpoint.enable(transition.changed_at),
            EndpointLifecycleAction::Revoke => endpoint.revoke(transition.changed_at),
        };
        result.map_err(RepositoryError::Conflict)?;
        let updated = AutomationWebhookEndpointRecord::new(endpoint, current.revision)
            .map_err(RepositoryError::Storage)?;
        state
            .endpoints
            .insert(transition.endpoint_id, updated.clone());
        Ok(updated)
    }

    async fn find_delivery(
        &self,
        endpoint_id: Uuid,
        delivery_id: Uuid,
    ) -> Result<Option<AutomationWebhookDeliveryRecord>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .deliveries
            .get(&(endpoint_id, delivery_id))
            .cloned())
    }

    async fn admit_delivery(
        &self,
        write: AdmitAutomationWebhookDeliveryWrite,
    ) -> Result<AutomationWebhookAdmission, RepositoryError> {
        let mut state = self.state.write().await;
        let endpoint_id = write.request.endpoint_id;
        let endpoint_record = state
            .endpoints
            .get(&endpoint_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        endpoint_record
            .validate()
            .map_err(RepositoryError::Storage)?;
        write
            .request
            .validate_for_endpoint(&endpoint_record.endpoint)
            .map_err(RepositoryError::Conflict)?;

        let delivery_key = (endpoint_id, write.request.delivery_id);
        if let Some(existing) = state.deliveries.get(&delivery_key).cloned() {
            let (receipt, invocation, replayed) = if existing.receipt.decision
                != AutomationWebhookAdmissionDecisionV1::Rejected
                && existing.request.body_digest == write.request.body_digest
            {
                let receipt = AutomationWebhookDeliveryReceiptV1::replay_of(
                    write.receipt_id,
                    &existing.receipt,
                    &endpoint_record.endpoint,
                    &endpoint_record.revision,
                    &write.request,
                    write.recorded_at,
                )
                .map_err(RepositoryError::Conflict)?;
                (receipt, existing.invocation.clone(), true)
            } else {
                let receipt = AutomationWebhookDeliveryReceiptV1::rejected(
                    write.receipt_id,
                    &endpoint_record.endpoint,
                    &endpoint_record.revision,
                    &write.request,
                    AutomationWebhookRejectionReasonV1::DuplicateDeliveryConflict,
                    write.recorded_at,
                )
                .map_err(RepositoryError::Conflict)?;
                (receipt, None, false)
            };
            let replay = AutomationWebhookDeliveryRecord::new(
                write.request,
                receipt,
                invocation,
                &endpoint_record.endpoint,
                &endpoint_record.revision,
            )
            .map_err(RepositoryError::Storage)?;
            record_receipt(&mut state, &replay, &endpoint_record, replayed)?;
            return Ok(AutomationWebhookAdmission {
                delivery: replay,
                replayed,
            });
        }

        if !endpoint_record.endpoint.state.is_accepting() {
            let reason = match endpoint_record.endpoint.state {
                a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Disabled => {
                    AutomationWebhookRejectionReasonV1::EndpointDisabled
                }
                a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Revoked => {
                    AutomationWebhookRejectionReasonV1::EndpointRevoked
                }
                a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Active => {
                    return Err(RepositoryError::Storage(
                        "inactive webhook endpoint state changed during admission".into(),
                    ))
                }
            };
            let receipt = AutomationWebhookDeliveryReceiptV1::rejected(
                write.receipt_id,
                &endpoint_record.endpoint,
                &endpoint_record.revision,
                &write.request,
                reason,
                write.recorded_at,
            )
            .map_err(RepositoryError::Conflict)?;
            let delivery = AutomationWebhookDeliveryRecord::new(
                write.request,
                receipt,
                None,
                &endpoint_record.endpoint,
                &endpoint_record.revision,
            )
            .map_err(RepositoryError::Storage)?;
            state.deliveries.insert(delivery.key(), delivery.clone());
            record_receipt(&mut state, &delivery, &endpoint_record, false)?;
            return Ok(AutomationWebhookAdmission {
                delivery,
                replayed: false,
            });
        }

        let invocation = write.invocation.ok_or_else(|| {
            RepositoryError::Conflict(
                "an active Automation webhook delivery requires an invocation envelope".into(),
            )
        })?;
        let receipt = AutomationWebhookDeliveryReceiptV1::admitted(
            write.receipt_id,
            &endpoint_record.endpoint,
            &endpoint_record.revision,
            &write.request,
            &invocation,
            write.recorded_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let delivery = AutomationWebhookDeliveryRecord::new(
            write.request,
            receipt,
            Some(invocation),
            &endpoint_record.endpoint,
            &endpoint_record.revision,
        )
        .map_err(RepositoryError::Storage)?;
        state.deliveries.insert(delivery.key(), delivery.clone());
        record_receipt(&mut state, &delivery, &endpoint_record, false)?;
        Ok(AutomationWebhookAdmission {
            delivery,
            replayed: false,
        })
    }
}

fn record_receipt(
    state: &mut State,
    delivery: &AutomationWebhookDeliveryRecord,
    endpoint: &AutomationWebhookEndpointRecord,
    replayed: bool,
) -> Result<(), RepositoryError> {
    state
        .receipts
        .insert(delivery.receipt.receipt_id, delivery.receipt.clone());
    if let Some(invocation) = &delivery.invocation {
        let action = if replayed {
            AutomationAuditActionV1::InvocationReplayed
        } else {
            AutomationAuditActionV1::InvocationAdmitted
        };
        let audit = AutomationAuditRecordV1::for_invocation(
            invocation,
            action,
            Uuid::now_v7(),
            delivery.receipt.recorded_at,
        )
        .map_err(RepositoryError::Storage)?;
        state.audit.push(audit);
        if !replayed && delivery.receipt.decision == AutomationWebhookAdmissionDecisionV1::Admitted
        {
            let outbox = AutomationOutboxMessageV1::for_invocation(
                invocation,
                Uuid::now_v7(),
                Some(delivery.request.delivery_id),
                delivery.receipt.recorded_at,
            )
            .map_err(RepositoryError::Storage)?;
            state.outbox.push(outbox);
        }
    }
    // Keep the exact revision binding checked at the transaction boundary even
    // for rejected receipts; this prevents a future adapter from persisting a
    // receipt against a mutable/latest revision.
    endpoint.validate().map_err(RepositoryError::Storage)
}
