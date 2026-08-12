use crate::modules::shared_kernel::domain::{RepositoryError, WorkflowDecisionId};
use crate::modules::workflow::domain::{
    FlowResumeDisposition, FlowResumeReceipt, HumanTaskResumeDelivery, IHumanTaskRepository,
};
use crate::modules::workflow::infrastructure::observe_flow_resume_receipt;
use a3s_flow::{FlowEngine, FlowError, FlowEvent, FlowEventEnvelope};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct HumanTaskResumeWorkerConfig {
    pub batch_size: usize,
    pub poll_interval: Duration,
    pub lease_duration: Duration,
    pub flow_operation_timeout: Duration,
    pub initial_backoff: Duration,
    pub maximum_backoff: Duration,
}

impl HumanTaskResumeWorkerConfig {
    pub fn validate(self) -> Result<Self, String> {
        let minimum_lease = self.flow_operation_timeout.saturating_mul(2);
        if self.batch_size == 0
            || self.poll_interval.is_zero()
            || self.flow_operation_timeout.is_zero()
            || self.lease_duration <= minimum_lease
            || self.initial_backoff.is_zero()
            || self.maximum_backoff < self.initial_backoff
        {
            return Err("HumanTask resume delivery requires a positive batch and timings, a lease longer than two Flow operations, and ordered backoff bounds".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskResumeFailure {
    pub workflow_decision_id: WorkflowDecisionId,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HumanTaskResumeReport {
    pub claimed: usize,
    pub delivered: usize,
    pub superseded: usize,
    pub retried: usize,
    pub conflicted: usize,
    pub failures: Vec<HumanTaskResumeFailure>,
}

pub struct HumanTaskResumeWorker {
    owner: Uuid,
    repository: Arc<dyn IHumanTaskRepository>,
    engine: FlowEngine,
    config: HumanTaskResumeWorkerConfig,
}

impl HumanTaskResumeWorker {
    pub fn new(
        repository: Arc<dyn IHumanTaskRepository>,
        engine: FlowEngine,
        config: HumanTaskResumeWorkerConfig,
    ) -> Result<Self, String> {
        Ok(Self {
            owner: Uuid::new_v4(),
            repository,
            engine,
            config: config.validate()?,
        })
    }

    pub async fn run_once(&self) -> Result<HumanTaskResumeReport, RepositoryError> {
        let deliveries = self
            .repository
            .claim_resume_deliveries(
                self.owner,
                self.config.batch_size,
                chrono::Utc::now(),
                self.config.lease_duration,
            )
            .await?;
        let mut report = HumanTaskResumeReport {
            claimed: deliveries.len(),
            ..HumanTaskResumeReport::default()
        };
        for delivery in deliveries {
            self.deliver(delivery, &mut report).await;
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once().await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    workflow_decision_id = %failure.workflow_decision_id,
                                    error = %failure.error,
                                    "HumanTask Flow resume delivery failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "HumanTask resume delivery claim failed"
                        ),
                    }
                }
            }
        }
    }

    async fn deliver(&self, delivery: HumanTaskResumeDelivery, report: &mut HumanTaskResumeReport) {
        let decision_id = delivery.record.decision.id;
        let result = self.resume_and_observe(&delivery).await;
        match result {
            Ok(receipt) => {
                let disposition = receipt.disposition();
                match self
                    .repository
                    .record_resume_receipt(
                        delivery.record.decision.organization_id,
                        decision_id,
                        self.owner,
                        receipt,
                        chrono::Utc::now(),
                    )
                    .await
                {
                    Ok(_) => match disposition {
                        FlowResumeDisposition::HookReceived => report.delivered += 1,
                        FlowResumeDisposition::RunTimedOut
                        | FlowResumeDisposition::RunCancelled => report.superseded += 1,
                    },
                    Err(error) => report.failures.push(HumanTaskResumeFailure {
                        workflow_decision_id: decision_id,
                        error: format!(
                            "Flow resume committed but its lease-bound receipt failed: {error}"
                        ),
                    }),
                }
            }
            Err(DeliveryFailure::Retry(error)) => {
                let retry_after = retry_delay(&self.config, delivery.attempt_count);
                match self
                    .repository
                    .retry_resume_delivery(
                        delivery.record.decision.organization_id,
                        decision_id,
                        self.owner,
                        &error,
                        chrono::Utc::now(),
                        retry_after,
                    )
                    .await
                {
                    Ok(()) => report.retried += 1,
                    Err(mark_error) => report.failures.push(HumanTaskResumeFailure {
                        workflow_decision_id: decision_id,
                        error: format!("{error}; could not schedule retry: {mark_error}"),
                    }),
                }
            }
            Err(DeliveryFailure::Conflict(error)) => {
                match self
                    .repository
                    .conflict_resume_delivery(
                        delivery.record.decision.organization_id,
                        decision_id,
                        self.owner,
                        &error,
                        chrono::Utc::now(),
                    )
                    .await
                {
                    Ok(()) => report.conflicted += 1,
                    Err(mark_error) => report.failures.push(HumanTaskResumeFailure {
                        workflow_decision_id: decision_id,
                        error: format!("{error}; could not record conflict: {mark_error}"),
                    }),
                }
            }
        }
    }

    async fn resume_and_observe(
        &self,
        delivery: &HumanTaskResumeDelivery,
    ) -> Result<crate::modules::workflow::domain::FlowResumeReceipt, DeliveryFailure> {
        let payload = &delivery.record.resume_payload;
        let flow_value = payload.to_flow_value().map_err(DeliveryFailure::Conflict)?;
        let resume = tokio::time::timeout(
            self.config.flow_operation_timeout,
            self.engine
                .resume_hook(&payload.flow_run_id, &payload.flow_hook_id, flow_value),
        )
        .await;
        let resume_error = match resume {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(_) => {
                return Err(DeliveryFailure::Retry(format!(
                    "Flow resume timed out after {} ms",
                    self.config.flow_operation_timeout.as_millis()
                )))
            }
        };

        let history = tokio::time::timeout(
            self.config.flow_operation_timeout,
            self.engine.history(&payload.flow_run_id),
        )
        .await
        .map_err(|_| {
            DeliveryFailure::Retry(format!(
                "Flow history read timed out after {} ms",
                self.config.flow_operation_timeout.as_millis()
            ))
        })?
        .map_err(|error| DeliveryFailure::Retry(error.to_string()))?;

        match matching_hook_received(&history, &payload.flow_hook_id) {
            Ok(Some(envelope)) => receipt_for_delivery(delivery, envelope),
            Ok(None) => {
                if let Some(envelope) = matching_run_timed_out(&history)? {
                    return receipt_for_delivery(delivery, envelope);
                }
                if let Some(envelope) = matching_run_cancelled(&history)? {
                    return receipt_for_delivery(delivery, envelope);
                }
                if history.iter().any(|envelope| {
                    matches!(
                        &envelope.event,
                        FlowEvent::HookDisposed { hook_id } if hook_id == &payload.flow_hook_id
                    )
                }) {
                    return Err(DeliveryFailure::Conflict(
                        "Flow hook was disposed before the WorkflowDecision could be delivered"
                            .into(),
                    ));
                }
                match resume_error {
                    Some(error) if permanent_flow_error(&error) => {
                        Err(DeliveryFailure::Conflict(error.to_string()))
                    }
                    Some(error) => Err(DeliveryFailure::Retry(error.to_string())),
                    None => Err(DeliveryFailure::Retry(
                        "Flow resume returned without durable HookReceived evidence".into(),
                    )),
                }
            }
            Err(error) => Err(DeliveryFailure::Conflict(error)),
        }
    }
}

fn receipt_for_delivery(
    delivery: &HumanTaskResumeDelivery,
    envelope: &FlowEventEnvelope,
) -> Result<FlowResumeReceipt, DeliveryFailure> {
    let receipt = observe_flow_resume_receipt(&delivery.record.resume_payload, envelope).map_err(
        |error| {
            DeliveryFailure::Conflict(format!(
                "Flow settlement evidence conflicts with its resume intent: {error}"
            ))
        },
    )?;
    let mut settled = delivery.record.clone();
    settled.resume_receipt = Some(receipt.clone());
    settled.validate().map_err(|error| {
        DeliveryFailure::Conflict(format!(
            "Flow settlement evidence conflicts with its HumanTask decision: {error}"
        ))
    })?;
    Ok(receipt)
}

#[derive(Debug)]
enum DeliveryFailure {
    Retry(String),
    Conflict(String),
}

fn matching_hook_received<'a>(
    history: &'a [FlowEventEnvelope],
    hook_id: &str,
) -> Result<Option<&'a FlowEventEnvelope>, String> {
    let matching = history
        .iter()
        .filter(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::HookReceived { hook_id: observed, .. } if observed == hook_id
            )
        })
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [] => Ok(None),
        [envelope] => Ok(Some(*envelope)),
        _ => Err(format!(
            "Flow history contains duplicate HookReceived events for hook {hook_id:?}"
        )),
    }
}

fn matching_run_timed_out(
    history: &[FlowEventEnvelope],
) -> Result<Option<&FlowEventEnvelope>, DeliveryFailure> {
    let matching = history
        .iter()
        .filter(|envelope| matches!(&envelope.event, FlowEvent::RunTimedOut { .. }))
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [] => Ok(None),
        [envelope] => Ok(Some(*envelope)),
        _ => Err(DeliveryFailure::Conflict(
            "Flow history contains duplicate RunTimedOut events".into(),
        )),
    }
}

fn matching_run_cancelled(
    history: &[FlowEventEnvelope],
) -> Result<Option<&FlowEventEnvelope>, DeliveryFailure> {
    let matching = history
        .iter()
        .filter(|envelope| matches!(&envelope.event, FlowEvent::RunCancelled { .. }))
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [] => Ok(None),
        [envelope] => Ok(Some(*envelope)),
        _ => Err(DeliveryFailure::Conflict(
            "Flow history contains duplicate RunCancelled events".into(),
        )),
    }
}

fn permanent_flow_error(error: &FlowError) -> bool {
    matches!(
        error,
        FlowError::InvalidRunId(_)
            | FlowError::RunTerminal(_)
            | FlowError::RunConflict { .. }
            | FlowError::NonDeterministic { .. }
            | FlowError::HookTokenNotFound(_)
            | FlowError::HookTokenConflict { .. }
            | FlowError::HookConflict { .. }
            | FlowError::InvalidWorkflow(_)
            | FlowError::InvalidTransition(_)
    )
}

fn retry_delay(config: &HumanTaskResumeWorkerConfig, attempts: u32) -> Duration {
    let exponent = attempts.saturating_sub(1).min(20);
    config
        .initial_backoff
        .saturating_mul(1_u32 << exponent)
        .min(config.maximum_backoff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{AuthorizationDecisionRef, Sha256Digest};
    use crate::modules::workflow::domain::{
        FlowResumePayload, HumanTaskDecisionRecord, HumanTaskInteractionSpec, HumanTaskRecord,
        WorkflowDecision,
    };
    use crate::modules::workflow::test_support::{pending_task, timestamp};
    use a3s_flow::FlowEvent;

    #[test]
    fn validates_worker_timings_and_bounds_retry_backoff() {
        let config = HumanTaskResumeWorkerConfig {
            batch_size: 10,
            poll_interval: Duration::from_millis(100),
            lease_duration: Duration::from_secs(10),
            flow_operation_timeout: Duration::from_secs(2),
            initial_backoff: Duration::from_millis(250),
            maximum_backoff: Duration::from_secs(2),
        };
        config.validate().expect("valid worker config");
        assert_eq!(retry_delay(&config, 1), Duration::from_millis(250));
        assert_eq!(retry_delay(&config, 2), Duration::from_millis(500));
        assert_eq!(retry_delay(&config, 20), Duration::from_secs(2));

        let mut invalid = config;
        invalid.lease_duration = Duration::from_secs(4);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn settles_expiry_delivery_from_the_exact_flow_timeout_only() {
        let (task, principal_id) = pending_task();
        let mut record = HumanTaskRecord::create(
            task,
            HumanTaskInteractionSpec::approval("Approve?", None, None).expect("interaction"),
            7,
            Uuid::now_v7(),
        )
        .expect("task record");
        record.activate(1, timestamp(8, 1)).expect("activation");
        let decision = WorkflowDecision::expire(
            WorkflowDecisionId::new(),
            &record.task,
            principal_id,
            AuthorizationDecisionRef::new(
                "deadline-authority",
                Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            )
            .expect("authority"),
            timestamp(10, 0),
        )
        .expect("expiry decision");
        record.expire(2, &decision).expect("expiry");
        let delivery = HumanTaskResumeDelivery {
            record: HumanTaskDecisionRecord {
                task: record,
                submission: None,
                resume_payload: FlowResumePayload::from_decision(&decision).expect("payload"),
                resume_receipt: None,
                decision,
            },
            attempt_count: 1,
            lease_owner: Uuid::now_v7(),
            claimed_at: timestamp(10, 1),
            lease_expires_at: timestamp(10, 2),
        };
        delivery.validate().expect("delivery");
        let timeout = FlowEventEnvelope {
            run_id: delivery.record.resume_payload.flow_run_id.clone(),
            sequence: 11,
            event_id: Uuid::now_v7(),
            timestamp: timestamp(10, 0),
            event: FlowEvent::RunTimedOut {
                deadline: timestamp(10, 0),
                reason: Some("WorkflowRun exceeded its immutable deadline".into()),
            },
        };
        let receipt = receipt_for_delivery(&delivery, &timeout).expect("terminal receipt");
        assert_eq!(receipt.disposition(), FlowResumeDisposition::RunTimedOut);

        let mut drifted = timeout;
        drifted.event = FlowEvent::RunTimedOut {
            deadline: timestamp(9, 59),
            reason: Some("different deadline".into()),
        };
        assert!(matches!(
            receipt_for_delivery(&delivery, &drifted),
            Err(DeliveryFailure::Conflict(_))
        ));
    }

    #[test]
    fn settles_parent_cancellation_delivery_from_the_exact_flow_event() {
        let (task, principal_id) = pending_task();
        let mut record = HumanTaskRecord::create(
            task,
            HumanTaskInteractionSpec::approval("Approve?", None, None).expect("interaction"),
            7,
            Uuid::now_v7(),
        )
        .expect("task record");
        record.activate(1, timestamp(8, 1)).expect("activation");
        let decision = WorkflowDecision::cancel(
            WorkflowDecisionId::new(),
            &record.task,
            principal_id,
            AuthorizationDecisionRef::new(
                "parent-cancellation-authority",
                Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("digest"),
            )
            .expect("authority"),
            timestamp(8, 4),
        )
        .expect("cancellation decision");
        record.cancel(2, &decision).expect("cancellation");
        let delivery = HumanTaskResumeDelivery {
            record: HumanTaskDecisionRecord {
                task: record,
                submission: None,
                resume_payload: FlowResumePayload::from_decision(&decision).expect("payload"),
                resume_receipt: None,
                decision,
            },
            attempt_count: 1,
            lease_owner: Uuid::now_v7(),
            claimed_at: timestamp(8, 5),
            lease_expires_at: timestamp(8, 6),
        };
        delivery.validate().expect("delivery");
        let cancelled = FlowEventEnvelope {
            run_id: delivery.record.resume_payload.flow_run_id.clone(),
            sequence: 11,
            event_id: Uuid::now_v7(),
            timestamp: timestamp(8, 4),
            event: FlowEvent::RunCancelled {
                reason: Some("operator request".into()),
            },
        };
        let receipt = receipt_for_delivery(&delivery, &cancelled).expect("terminal receipt");
        assert_eq!(receipt.disposition(), FlowResumeDisposition::RunCancelled);

        let mut too_early = cancelled;
        too_early.timestamp = timestamp(8, 3);
        assert!(matches!(
            receipt_for_delivery(&delivery, &too_early),
            Err(DeliveryFailure::Conflict(_))
        ));
    }
}
