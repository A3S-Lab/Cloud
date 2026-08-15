use crate::modules::connectors::{
    ConnectorExecutionApplicationService, ConnectorExecutionAttemptResult,
    ConnectorExecutionEvidence, ConnectorExecutionOutcome, ExecuteConnectorAttempt,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::domain::{
    outbound_notification_attempt_id, IOutboundNotificationRequestAdapter,
    OutboundNotificationChannel, OutboundNotificationDelivery,
    MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
};
use crate::modules::notifications::infrastructure::{
    SignedWebhookNotificationAdapter, SlackCompatibleNotificationAdapter,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::canonical_timestamp;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub trait IOutboundNotificationDispatcher: Send + Sync {
    async fn dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        delivery_count: u64,
    ) -> ApplicationResult<OutboundNotificationDispatchResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundNotificationDispatchResult {
    Delivered {
        generation: u64,
        evidence: ConnectorExecutionEvidence,
    },
    Rejected {
        generation: u64,
        evidence: ConnectorExecutionEvidence,
    },
    Retryable {
        generation: u64,
        evidence: ConnectorExecutionEvidence,
    },
    Exhausted {
        generation: u64,
        evidence: ConnectorExecutionEvidence,
    },
    Deferred {
        generation: u64,
        attempt_id: Uuid,
        retry_not_before: DateTime<Utc>,
    },
    Indeterminate {
        generation: u64,
        attempt_id: Uuid,
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    },
}

/// Composes channel request builders with the sole fenced Connector execution service.
///
/// `delivery_count` comes from A3S Event. A new Connector attempt generation is admitted
/// only after every preceding generation has durable `retryable` evidence. Replaying an
/// accepted/rejected generation therefore never crosses the provider boundary again.
pub struct OutboundNotificationDispatcher {
    connectors: Arc<ConnectorExecutionApplicationService>,
    signed_webhook: Arc<dyn IOutboundNotificationRequestAdapter>,
    slack_compatible: Arc<dyn IOutboundNotificationRequestAdapter>,
}

impl OutboundNotificationDispatcher {
    pub fn new(connectors: Arc<ConnectorExecutionApplicationService>) -> Self {
        Self::with_adapters(
            connectors,
            Arc::new(SignedWebhookNotificationAdapter::new()),
            Arc::new(SlackCompatibleNotificationAdapter::new()),
        )
    }

    pub fn with_adapters(
        connectors: Arc<ConnectorExecutionApplicationService>,
        signed_webhook: Arc<dyn IOutboundNotificationRequestAdapter>,
        slack_compatible: Arc<dyn IOutboundNotificationRequestAdapter>,
    ) -> Self {
        Self {
            connectors,
            signed_webhook,
            slack_compatible,
        }
    }

    async fn dispatch_fenced(
        &self,
        delivery: &OutboundNotificationDelivery,
        delivery_count: u64,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        delivery.validate().map_err(ApplicationError::Invalid)?;
        if delivery_count == 0 {
            return Err(ApplicationError::Invalid(
                "A3S Event delivery count must be positive".into(),
            ));
        }
        // A3S Event's delivery count includes transport/infrastructure redeliveries;
        // it is not itself a Connector retry counter. Keep accepting those
        // redeliveries while bounding the number of logical provider attempts.
        let maximum_generation = delivery_count
            .min(MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION)
            .min(MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS);
        let adapter = self.adapter(delivery.channel())?;
        let target = delivery.target();
        let resource_access =
            ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
                project_id: target.project_id,
                environment_id: target.environment_id,
            }]);

        for generation in 1..=maximum_generation {
            let attempt_id = outbound_notification_attempt_id(delivery.id(), generation)
                .map_err(ApplicationError::Invalid)?;
            let request = adapter
                .build_request(delivery, attempt_id)
                .map_err(|error| ApplicationError::Invalid(error.to_string()))?;
            let result = self
                .connectors
                .execute(ExecuteConnectorAttempt {
                    organization_id: delivery.organization_id(),
                    project_id: target.project_id,
                    environment_id: target.environment_id,
                    profile_id: target.profile_id,
                    revision_id: target.revision_id,
                    request,
                    resource_access: resource_access.clone(),
                    fence_token: Uuid::now_v7(),
                    requested_at: canonical_timestamp(Utc::now()),
                })
                .await?;
            match result {
                ConnectorExecutionAttemptResult::Completed {
                    evidence, replayed, ..
                } => match evidence.outcome() {
                    ConnectorExecutionOutcome::Accepted => {
                        return Ok(OutboundNotificationDispatchResult::Delivered {
                            generation,
                            evidence,
                        })
                    }
                    ConnectorExecutionOutcome::Rejected => {
                        return Ok(OutboundNotificationDispatchResult::Rejected {
                            generation,
                            evidence,
                        })
                    }
                    ConnectorExecutionOutcome::Retryable
                        if replayed
                            && generation == MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS =>
                    {
                        return Ok(OutboundNotificationDispatchResult::Exhausted {
                            generation,
                            evidence,
                        })
                    }
                    ConnectorExecutionOutcome::Retryable
                        if replayed && generation < maximum_generation =>
                    {
                        if let Some(deferred) = defer_retryable_replay(
                            &evidence,
                            generation,
                            attempt_id,
                            canonical_timestamp(Utc::now()),
                        )? {
                            return Ok(deferred);
                        }
                    }
                    ConnectorExecutionOutcome::Retryable => {
                        return Ok(OutboundNotificationDispatchResult::Retryable {
                            generation,
                            evidence,
                        })
                    }
                },
                ConnectorExecutionAttemptResult::SettlementPending { settlement, .. } => {
                    let settled = self
                        .connectors
                        .settle_known(settlement, &resource_access)
                        .await?;
                    match settled {
                        ConnectorExecutionAttemptResult::Completed {
                            evidence, replayed, ..
                        } => match evidence.outcome() {
                            ConnectorExecutionOutcome::Accepted => {
                                return Ok(OutboundNotificationDispatchResult::Delivered {
                                    generation,
                                    evidence,
                                })
                            }
                            ConnectorExecutionOutcome::Rejected => {
                                return Ok(OutboundNotificationDispatchResult::Rejected {
                                    generation,
                                    evidence,
                                })
                            }
                            ConnectorExecutionOutcome::Retryable
                                if replayed
                                    && generation
                                        == MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS =>
                            {
                                return Ok(OutboundNotificationDispatchResult::Exhausted {
                                    generation,
                                    evidence,
                                })
                            }
                            ConnectorExecutionOutcome::Retryable
                                if replayed && generation < maximum_generation =>
                            {
                                if let Some(deferred) = defer_retryable_replay(
                                    &evidence,
                                    generation,
                                    attempt_id,
                                    canonical_timestamp(Utc::now()),
                                )? {
                                    return Ok(deferred);
                                }
                            }
                            ConnectorExecutionOutcome::Retryable => {
                                return Ok(OutboundNotificationDispatchResult::Retryable {
                                    generation,
                                    evidence,
                                })
                            }
                        },
                        _ => {
                            return Err(ApplicationError::Internal(
                                "known Connector settlement did not become terminal".into(),
                            ))
                        }
                    }
                }
                ConnectorExecutionAttemptResult::Reserved { lease_expires_at }
                | ConnectorExecutionAttemptResult::ReservationExpired { lease_expires_at } => {
                    return Ok(OutboundNotificationDispatchResult::Deferred {
                        generation,
                        attempt_id,
                        retry_not_before: lease_expires_at,
                    })
                }
                ConnectorExecutionAttemptResult::InFlight {
                    outcome_deadline_at,
                    ..
                } => {
                    return Ok(OutboundNotificationDispatchResult::Deferred {
                        generation,
                        attempt_id,
                        retry_not_before: outcome_deadline_at,
                    })
                }
                ConnectorExecutionAttemptResult::Indeterminate {
                    dispatch_started_at,
                    outcome_deadline_at,
                } => {
                    return Ok(OutboundNotificationDispatchResult::Indeterminate {
                        generation,
                        attempt_id,
                        dispatch_started_at,
                        outcome_deadline_at,
                    })
                }
            }
        }

        Err(ApplicationError::Internal(
            "outbound notification dispatch exhausted no generation".into(),
        ))
    }

    fn adapter(
        &self,
        channel: OutboundNotificationChannel,
    ) -> ApplicationResult<&dyn IOutboundNotificationRequestAdapter> {
        match channel {
            OutboundNotificationChannel::SignedWebhook => Ok(&*self.signed_webhook),
            OutboundNotificationChannel::SlackCompatible => Ok(&*self.slack_compatible),
            OutboundNotificationChannel::Smtp => Err(ApplicationError::Unavailable(
                "SMTP notification delivery is unavailable without an Identity-owned verified contact reference"
                    .into(),
            )),
        }
    }
}

fn defer_retryable_replay(
    evidence: &ConnectorExecutionEvidence,
    generation: u64,
    attempt_id: Uuid,
    observed_at: DateTime<Utc>,
) -> ApplicationResult<Option<OutboundNotificationDispatchResult>> {
    let Some(retry_after) = evidence.retry_after() else {
        return Ok(None);
    };
    let retry_after = chrono::Duration::from_std(retry_after).map_err(|_| {
        ApplicationError::Internal("Connector Retry-After exceeds chrono bounds".into())
    })?;
    let retry_not_before = evidence
        .completed_at()
        .checked_add_signed(retry_after)
        .map(canonical_timestamp)
        .ok_or_else(|| {
            ApplicationError::Internal("Connector Retry-After deadline overflowed".into())
        })?;
    if observed_at < retry_not_before {
        return Ok(Some(OutboundNotificationDispatchResult::Deferred {
            generation,
            attempt_id,
            retry_not_before,
        }));
    }
    Ok(None)
}

#[async_trait]
impl IOutboundNotificationDispatcher for OutboundNotificationDispatcher {
    async fn dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        delivery_count: u64,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        self.dispatch_fenced(delivery, delivery_count).await
    }
}

#[cfg(test)]
#[path = "outbound_dispatch_tests.rs"]
mod tests;
