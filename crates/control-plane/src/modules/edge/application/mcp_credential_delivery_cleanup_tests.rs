use super::*;
use crate::modules::edge::domain::repositories::{
    McpCredentialLifecycleResult, StoreMcpCredentialLifecycle,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId,
};
use async_trait::async_trait;
use chrono::TimeZone;
use std::collections::VecDeque;
use tokio::sync::Mutex;

struct CleanupRepository {
    calls: Mutex<Vec<(DateTime<Utc>, usize)>>,
    results: Mutex<VecDeque<Result<usize, RepositoryError>>>,
}

#[async_trait]
impl IMcpCredentialLifecycleRepository for CleanupRepository {
    async fn replay_mcp_credential_lifecycle(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _idempotency: &IdempotencyRequest,
        _observed_at: DateTime<Utc>,
    ) -> Result<Option<McpCredentialLifecycleResult>, RepositoryError> {
        Err(RepositoryError::Storage(
            "unexpected lifecycle replay".into(),
        ))
    }

    async fn store_mcp_credential_lifecycle(
        &self,
        _bundle: StoreMcpCredentialLifecycle,
    ) -> Result<McpCredentialLifecycleResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "unexpected lifecycle store".into(),
        ))
    }

    async fn purge_expired_mcp_credential_deliveries(
        &self,
        observed_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        self.calls.lock().await.push((observed_at, limit));
        self.results.lock().await.pop_front().unwrap_or(Ok(0))
    }
}

#[tokio::test]
async fn cleanup_is_bounded_canonical_and_retryable() {
    let failure = RepositoryError::Storage("injected cleanup interruption".into());
    let repository = Arc::new(CleanupRepository {
        calls: Mutex::new(Vec::new()),
        results: Mutex::new(VecDeque::from([Err(failure.clone()), Ok(7)])),
    });
    let repository_port: Arc<dyn IMcpCredentialLifecycleRepository> = repository.clone();
    let worker =
        McpCredentialDeliveryCleanupWorker::new(repository_port, Duration::from_secs(60), 256)
            .expect("worker");
    let observed_at = Utc
        .with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
        .single()
        .expect("time")
        + chrono::Duration::nanoseconds(123_456_789);

    assert_eq!(worker.run_once(observed_at).await, Err(failure));
    assert_eq!(worker.run_once(observed_at).await.expect("retry"), 7);

    let calls = repository.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], (canonical_timestamp(observed_at), 256));
    assert_eq!(calls[1], (canonical_timestamp(observed_at), 256));
}

#[test]
fn cleanup_policy_rejects_unbounded_or_busy_loops() {
    let repository: Arc<dyn IMcpCredentialLifecycleRepository> = Arc::new(CleanupRepository {
        calls: Mutex::new(Vec::new()),
        results: Mutex::new(VecDeque::new()),
    });
    assert!(
        McpCredentialDeliveryCleanupWorker::new(Arc::clone(&repository), Duration::ZERO, 1,)
            .is_err()
    );
    assert!(McpCredentialDeliveryCleanupWorker::new(
        Arc::clone(&repository),
        Duration::from_secs(1),
        0,
    )
    .is_err());
    assert!(McpCredentialDeliveryCleanupWorker::new(
        repository,
        Duration::from_secs(1),
        MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH + 1,
    )
    .is_err());
}
