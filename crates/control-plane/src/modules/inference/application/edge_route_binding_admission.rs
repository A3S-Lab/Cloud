//! Fail-closed EdgeRouteBinding admission for Inference route publication.
//!
//! Edge remains authoritative for DomainClaim and GatewayScope. Inference owns
//! only the binding reference and admits publication through this port.

use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use async_trait::async_trait;

/// Stable Inference publication error code for rejected Edge bindings.
pub const EDGE_ROUTE_BINDING_INVALID: &str = "EDGE_ROUTE_BINDING_INVALID";

/// Same-environment binding admission request for `PublishInferenceRoute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceEdgeRouteBindingAdmissionRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub binding: EdgeRouteBindingRef,
}

impl InferenceEdgeRouteBindingAdmissionRequest {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        binding: EdgeRouteBindingRef,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            binding,
        }
    }
}

/// Consumer-owned port: Edge implements admission; Inference never loads Edge
/// entities inside its domain layer.
#[async_trait]
pub trait IInferenceEdgeRouteBindingAdmissionPort: Send + Sync {
    async fn admit(
        &self,
        request: InferenceEdgeRouteBindingAdmissionRequest,
    ) -> ApplicationResult<()>;
}

/// Test/unit fixture that admits every binding without consulting Edge.
#[derive(Debug, Clone, Copy, Default)]
pub struct PermitInferenceEdgeRouteBindingAdmission;

#[async_trait]
impl IInferenceEdgeRouteBindingAdmissionPort for PermitInferenceEdgeRouteBindingAdmission {
    async fn admit(
        &self,
        _request: InferenceEdgeRouteBindingAdmissionRequest,
    ) -> ApplicationResult<()> {
        Ok(())
    }
}
