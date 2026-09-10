//! Fail-closed grant→credential admission for Inference route publication.
//!
//! Identity remains authoritative for inference credentials. Inference owns
//! only grant references and admits publication through this port.

use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_cloud_contracts::InferenceGrantAclProjection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Stable Inference publication error code for rejected grant credentials.
pub const INFERENCE_GRANT_CREDENTIAL_INVALID: &str = "INFERENCE_GRANT_CREDENTIAL_INVALID";

/// Same-environment grant admission request for `PublishInferenceRoute`
/// and `ReviseInferenceRoute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceGrantCredentialAdmissionRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub grants: Vec<InferenceGrantAclProjection>,
    pub observed_at: DateTime<Utc>,
}

impl InferenceGrantCredentialAdmissionRequest {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        grants: Vec<InferenceGrantAclProjection>,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            grants,
            observed_at,
        }
    }
}

/// Consumer-owned port: Identity implements admission; Inference never loads
/// Identity credential entities inside its domain layer.
#[async_trait]
pub trait IInferenceGrantCredentialAdmissionPort: Send + Sync {
    async fn admit(
        &self,
        request: InferenceGrantCredentialAdmissionRequest,
    ) -> ApplicationResult<()>;
}

/// Test/unit fixture that admits every grant without consulting Identity.
#[derive(Debug, Clone, Copy, Default)]
pub struct PermitInferenceGrantCredentialAdmission;

#[async_trait]
impl IInferenceGrantCredentialAdmissionPort for PermitInferenceGrantCredentialAdmission {
    async fn admit(
        &self,
        _request: InferenceGrantCredentialAdmissionRequest,
    ) -> ApplicationResult<()> {
        Ok(())
    }
}
