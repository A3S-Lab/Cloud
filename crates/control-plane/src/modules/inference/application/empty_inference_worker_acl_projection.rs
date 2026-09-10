//! Empty Inference worker ACL projection until Power observation delivery.

use super::{IInferenceWorkerAclProjectionPort, InferenceRouteEnvironmentScope};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceWorkerAclProjection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Returns no worker projections. Preserves the worker-empty inference shell
/// when Edge compiles managed snapshots without Power observation delivery.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyInferenceWorkerAclProjectionPort;

/// Honest Postgres (and other production) composition until Power observation
/// delivery exists. Edge must load workers only through this port surface —
/// never invent workers from route catalogs or Workloads replicas.
pub fn postgres_inference_worker_acl_projections() -> Arc<dyn IInferenceWorkerAclProjectionPort> {
    Arc::new(EmptyInferenceWorkerAclProjectionPort)
}

#[async_trait]
impl IInferenceWorkerAclProjectionPort for EmptyInferenceWorkerAclProjectionPort {
    async fn list_inference_worker_acl_projections(
        &self,
        _scopes: &[InferenceRouteEnvironmentScope],
        _projected_at: DateTime<Utc>,
    ) -> Result<Vec<InferenceWorkerAclProjection>, RepositoryError> {
        Ok(Vec::new())
    }
}
