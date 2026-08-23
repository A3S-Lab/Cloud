use crate::modules::fleet::domain::repositories::{
    INodeAvailabilityRepository, NodeAvailabilityReconciliationResult, ReconcileNodeAvailability,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub struct NodeAvailabilityReconciler {
    repository: Arc<dyn INodeAvailabilityRepository>,
    reconcile_interval: Duration,
    heartbeat_timeout: chrono::Duration,
    batch_size: usize,
}

impl NodeAvailabilityReconciler {
    pub fn new(
        repository: Arc<dyn INodeAvailabilityRepository>,
        reconcile_interval: Duration,
        heartbeat_timeout: chrono::Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if reconcile_interval.is_zero()
            || heartbeat_timeout <= chrono::Duration::zero()
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err("Node availability reconciliation policy is invalid".into());
        }
        Ok(Self {
            repository,
            reconcile_interval,
            heartbeat_timeout,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        evaluated_at: DateTime<Utc>,
    ) -> Result<NodeAvailabilityReconciliationResult, RepositoryError> {
        self.repository
            .reconcile_node_availability(ReconcileNodeAvailability {
                evaluated_at,
                heartbeat_timeout: self.heartbeat_timeout,
                limit: self.batch_size,
            })
            .await
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.reconcile_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(result) => tracing::debug!(
                            processed_nodes = result.processed_nodes,
                            initialized_heads = result.initialized_heads,
                            unavailable_facts = result.unavailable_facts,
                            "Node availability reconciliation cycle completed"
                        ),
                        Err(error) => tracing::error!(
                            error = %error,
                            "Node availability reconciliation cycle failed"
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
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingRepository {
        request: Mutex<Option<ReconcileNodeAvailability>>,
    }

    #[async_trait]
    impl INodeAvailabilityRepository for RecordingRepository {
        async fn reconcile_node_availability(
            &self,
            request: ReconcileNodeAvailability,
        ) -> Result<NodeAvailabilityReconciliationResult, RepositoryError> {
            *self.request.lock().expect("request lock") = Some(request);
            Ok(NodeAvailabilityReconciliationResult {
                processed_nodes: 3,
                initialized_heads: 2,
                unavailable_facts: 1,
            })
        }
    }

    #[tokio::test]
    async fn run_once_delegates_one_bounded_acl_timed_page() {
        let repository = Arc::new(RecordingRepository::default());
        let reconciler = NodeAvailabilityReconciler::new(
            repository.clone(),
            Duration::from_secs(5),
            chrono::Duration::seconds(30),
            128,
        )
        .expect("reconciler");
        let evaluated_at = Utc::now();
        assert_eq!(
            reconciler.run_once(evaluated_at).await.expect("run once"),
            NodeAvailabilityReconciliationResult {
                processed_nodes: 3,
                initialized_heads: 2,
                unavailable_facts: 1,
            }
        );
        assert_eq!(
            *repository.request.lock().expect("request lock"),
            Some(ReconcileNodeAvailability {
                evaluated_at,
                heartbeat_timeout: chrono::Duration::seconds(30),
                limit: 128,
            })
        );
    }

    #[test]
    fn constructor_rejects_unbounded_or_nonpositive_policy() {
        let repository: Arc<dyn INodeAvailabilityRepository> =
            Arc::new(RecordingRepository::default());
        assert!(NodeAvailabilityReconciler::new(
            Arc::clone(&repository),
            Duration::ZERO,
            chrono::Duration::seconds(30),
            1,
        )
        .is_err());
        assert!(NodeAvailabilityReconciler::new(
            Arc::clone(&repository),
            Duration::from_secs(1),
            chrono::Duration::zero(),
            1,
        )
        .is_err());
        assert!(NodeAvailabilityReconciler::new(
            repository,
            Duration::from_secs(1),
            chrono::Duration::seconds(30),
            10_001,
        )
        .is_err());
    }

    #[test]
    fn production_composition_is_worker_only_and_reuses_the_fleet_acl() {
        let application = include_str!("../../../app.rs");
        let server = include_str!("../../../server.rs");
        let adapters = include_str!("../../../app/postgres_adapters.rs");
        let worker = application
            .split_once("let worker_processes = if let Some(flow) = flow.as_ref() {")
            .and_then(|(_, tail)| tail.split_once("let readiness = match management.as_ref()"))
            .map(|(body, _)| body)
            .expect("Worker composition branch");

        assert_eq!(
            application
                .matches("NodeAvailabilityReconciler::new(")
                .count(),
            1
        );
        for required in [
            "NodeAvailabilityReconciler::new(",
            "config.fleet.heartbeat_interval_ms",
            "config.fleet.heartbeat_timeout_ms",
            "node_availability_reconciler",
        ] {
            assert!(worker.contains(required), "Worker lost {required}");
        }
        assert_eq!(
            server.matches("\"Node availability reconciler\"").count(),
            1
        );
        assert!(server.contains("node_availability_reconciler.run(shutdown)"));
        assert!(adapters.contains("Arc<dyn INodeAvailabilityRepository>"));
        assert!(!application.contains("node_availability_poll_interval"));
        assert!(!application.contains("notification_node_availability"));
    }
}
