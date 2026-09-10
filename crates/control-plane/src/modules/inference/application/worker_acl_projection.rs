//! Narrow Inference owner query for Edge Gateway worker ACL projection.
//!
//! Edge receives only contract-level worker projections. Power observation
//! delivery and catalog internals stay private to Inference (+ Power). Until
//! observation delivery is active, [`EmptyInferenceWorkerAclProjectionPort`]
//! is the honest composition.

use super::InferenceRouteEnvironmentScope;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceWorkerAclProjection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Inference-owned projection port for managed Gateway inference worker ACL.
///
/// Scopes reuse [`InferenceRouteEnvironmentScope`] because workers are loaded
/// for the same org/project/environment identities as route grants. Postgres
/// composition wires Empty until Power observation delivery exists.
#[async_trait]
pub trait IInferenceWorkerAclProjectionPort: Send + Sync {
    async fn list_inference_worker_acl_projections(
        &self,
        scopes: &[InferenceRouteEnvironmentScope],
        projected_at: DateTime<Utc>,
    ) -> Result<Vec<InferenceWorkerAclProjection>, RepositoryError>;
}
