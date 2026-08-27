use crate::modules::agents::domain::{
    AgentExecutionCheckpointObjectLease, AgentExecutionCheckpointObjectReconcileDisposition,
    ClaimExpiredAgentExecutionCheckpointObjectsWrite,
    CompleteAgentExecutionCheckpointObjectCleanupWrite, IAgentExecutionCheckpointObjectStore,
    IAgentRepository, ReconcileAgentExecutionCheckpointObjectWrite,
    MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_LEASE_MS,
    MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_ORPHAN_GRACE_MS,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentExecutionCheckpointObjectReconcileReport {
    pub expired_claims: usize,
    pub inventoried: usize,
    pub referenced: usize,
    pub deferred: usize,
    pub removed: usize,
    pub malformed: usize,
    pub failures: Vec<String>,
}

pub struct AgentExecutionCheckpointObjectReconciler {
    agents: Arc<dyn IAgentRepository>,
    objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
    interval: Duration,
    orphan_grace: ChronoDuration,
    cleanup_lease_duration: ChronoDuration,
    batch_size: usize,
    cursor: Mutex<Option<String>>,
}

impl AgentExecutionCheckpointObjectReconciler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
        interval: Duration,
        orphan_grace: ChronoDuration,
        cleanup_lease_duration: ChronoDuration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero()
            || interval > Duration::from_secs(60 * 60)
            || orphan_grace <= ChronoDuration::zero()
            || orphan_grace
                > ChronoDuration::milliseconds(
                    MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_ORPHAN_GRACE_MS as i64,
                )
            || cleanup_lease_duration <= ChronoDuration::zero()
            || cleanup_lease_duration
                > ChronoDuration::milliseconds(
                    MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_LEASE_MS as i64,
                )
            || batch_size == 0
            || batch_size > 1_000
        {
            return Err(
                "Agent checkpoint object reconciliation schedule or bound is invalid".into(),
            );
        }
        Ok(Self {
            agents,
            objects,
            interval,
            orphan_grace,
            cleanup_lease_duration,
            batch_size,
            cursor: Mutex::new(None),
        })
    }

    pub async fn run_once(
        &self,
    ) -> Result<AgentExecutionCheckpointObjectReconcileReport, RepositoryError> {
        self.run_once_at(canonical_timestamp(Utc::now())).await
    }

    pub async fn run_once_at(
        &self,
        observed_at: DateTime<Utc>,
    ) -> Result<AgentExecutionCheckpointObjectReconcileReport, RepositoryError> {
        let observed_at = canonical_timestamp(observed_at);
        let mut report = AgentExecutionCheckpointObjectReconcileReport::default();
        let claims = self
            .agents
            .claim_expired_execution_checkpoint_objects(
                ClaimExpiredAgentExecutionCheckpointObjectsWrite {
                    claimed_at: observed_at,
                    cleanup_lease_duration: self.cleanup_lease_duration,
                    limit: self.batch_size,
                },
            )
            .await?;
        report.expired_claims = claims.len();
        for claim in claims {
            self.cleanup(claim, observed_at, &mut report).await;
        }

        let after = self.cursor.lock().await.clone();
        let page = match self
            .objects
            .inventory_page(after.as_deref(), self.batch_size)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                report.failures.push(format!(
                    "could not inventory Agent checkpoint objects: {error}"
                ));
                return Ok(report);
            }
        };
        *self.cursor.lock().await = page.next_after.clone();
        for entry in page.entries {
            report.inventoried += 1;
            let reference = match entry.reference() {
                Ok(reference) => reference,
                Err(error) => {
                    report.malformed += 1;
                    report.failures.push(format!(
                        "Agent checkpoint object {:?} has an invalid inventory identity: {error}",
                        entry.object_ref
                    ));
                    continue;
                }
            };
            match self
                .agents
                .reconcile_execution_checkpoint_object(
                    ReconcileAgentExecutionCheckpointObjectWrite {
                        reference,
                        observed_at,
                        orphan_grace: self.orphan_grace,
                        cleanup_lease_duration: self.cleanup_lease_duration,
                    },
                )
                .await
            {
                Ok(AgentExecutionCheckpointObjectReconcileDisposition::Referenced) => {
                    report.referenced += 1;
                }
                Ok(AgentExecutionCheckpointObjectReconcileDisposition::Deferred { .. }) => {
                    report.deferred += 1;
                }
                Ok(AgentExecutionCheckpointObjectReconcileDisposition::CleanupClaimed(lease)) => {
                    self.cleanup(*lease, observed_at, &mut report).await;
                }
                Err(error) => report.failures.push(format!(
                    "could not reconcile Agent checkpoint object inventory: {error}"
                )),
            }
        }
        Ok(report)
    }

    async fn cleanup(
        &self,
        lease: AgentExecutionCheckpointObjectLease,
        completed_at: DateTime<Utc>,
        report: &mut AgentExecutionCheckpointObjectReconcileReport,
    ) {
        if let Err(error) = self.objects.remove(&lease.reference).await {
            report.failures.push(format!(
                "could not remove orphan Agent checkpoint object {}: {error}",
                lease.reference.object_ref
            ));
            return;
        }
        match self
            .agents
            .complete_execution_checkpoint_object_cleanup(
                CompleteAgentExecutionCheckpointObjectCleanupWrite {
                    lease,
                    completed_at,
                },
            )
            .await
        {
            Ok(()) => report.removed += 1,
            Err(error) => report.failures.push(format!(
                "could not complete Agent checkpoint object cleanup: {error}"
            )),
        }
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
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
                            for error in report.failures {
                                tracing::warn!(error = %error, "Agent checkpoint object reconciliation failed");
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Agent checkpoint object lease scan failed"
                        ),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agents::domain::{
        AgentExecutionCheckpointObjectError, AgentExecutionCheckpointObjectInventoryEntry,
        AgentExecutionCheckpointObjectInventoryPage, AgentExecutionCheckpointObjectReference,
        AgentExecutionCheckpointObjectWrite,
    };
    use crate::modules::agents::infrastructure::InMemoryAgentRepository;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    use tokio::sync::RwLock;
    use uuid::Uuid;

    #[derive(Default)]
    struct InventoryObjects {
        objects: RwLock<BTreeMap<String, u64>>,
    }

    #[async_trait]
    impl IAgentExecutionCheckpointObjectStore for InventoryObjects {
        async fn put(
            &self,
            _reference: &AgentExecutionCheckpointObjectReference,
            _body: Vec<u8>,
        ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError>
        {
            unreachable!("worker test does not write checkpoint bodies")
        }

        async fn get(
            &self,
            _reference: &AgentExecutionCheckpointObjectReference,
        ) -> Result<Vec<u8>, AgentExecutionCheckpointObjectError> {
            unreachable!("worker test does not read checkpoint bodies")
        }

        async fn inventory_page(
            &self,
            after: Option<&str>,
            limit: usize,
        ) -> Result<AgentExecutionCheckpointObjectInventoryPage, AgentExecutionCheckpointObjectError>
        {
            let objects = self.objects.read().await;
            let mut entries = objects
                .iter()
                .filter(|(key, _)| after.is_none_or(|after| key.as_str() > after))
                .take(limit + 1)
                .map(
                    |(object_ref, size_bytes)| AgentExecutionCheckpointObjectInventoryEntry {
                        object_ref: object_ref.clone(),
                        size_bytes: *size_bytes,
                    },
                )
                .collect::<Vec<_>>();
            let has_more = entries.len() > limit;
            entries.truncate(limit);
            let next_after = has_more
                .then(|| entries.last().map(|entry| entry.object_ref.clone()))
                .flatten();
            Ok(AgentExecutionCheckpointObjectInventoryPage {
                entries,
                next_after,
            })
        }

        async fn remove(
            &self,
            reference: &AgentExecutionCheckpointObjectReference,
        ) -> Result<(), AgentExecutionCheckpointObjectError> {
            self.objects.write().await.remove(&reference.object_ref);
            Ok(())
        }
    }

    fn reference() -> AgentExecutionCheckpointObjectReference {
        let digest = Sha256Digest::from_bytes(b"orphan");
        let digest_hex = digest
            .as_str()
            .strip_prefix("sha256:")
            .expect("digest prefix");
        AgentExecutionCheckpointObjectReference::from_inventory(
            format!(
                "organizations/{}/executions/{}/checkpoints/{}/sha256/{digest_hex}/checkpoint.json",
                Uuid::now_v7(),
                Uuid::now_v7(),
                Uuid::now_v7()
            ),
            6,
        )
        .expect("inventory reference")
    }

    #[tokio::test]
    async fn inventory_grace_then_expired_lease_cleanup_is_exact_and_idempotent() {
        let agents: Arc<dyn IAgentRepository> = Arc::new(InMemoryAgentRepository::new());
        let objects = Arc::new(InventoryObjects::default());
        let reference = reference();
        objects
            .objects
            .write()
            .await
            .insert(reference.object_ref.clone(), reference.size_bytes);
        let reconciler = AgentExecutionCheckpointObjectReconciler::new(
            agents,
            objects.clone(),
            Duration::from_secs(1),
            ChronoDuration::seconds(10),
            ChronoDuration::seconds(5),
            100,
        )
        .expect("reconciler");
        let now = canonical_timestamp(Utc::now());
        let observed = reconciler.run_once_at(now).await.expect("inventory pass");
        assert_eq!(observed.deferred, 1);
        assert_eq!(observed.removed, 0);
        assert!(objects
            .objects
            .read()
            .await
            .contains_key(&reference.object_ref));

        let cleaned = reconciler
            .run_once_at(now + ChronoDuration::seconds(11))
            .await
            .expect("cleanup pass");
        assert_eq!(cleaned.expired_claims, 1);
        assert_eq!(cleaned.removed, 1);
        assert!(!objects
            .objects
            .read()
            .await
            .contains_key(&reference.object_ref));

        let replay = reconciler
            .run_once_at(now + ChronoDuration::seconds(20))
            .await
            .expect("empty replay pass");
        assert_eq!(replay.removed, 0);
        assert!(replay.failures.is_empty());
    }
}
