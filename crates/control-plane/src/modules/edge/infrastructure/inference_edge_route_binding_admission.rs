//! Edge-owned admission for Inference `EdgeRouteBindingRef` publication.

use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::{DomainClaimState, RouteHostname};
use crate::modules::inference::application::{
    InferenceEdgeRouteBindingAdmissionRequest, IInferenceEdgeRouteBindingAdmissionPort,
    EDGE_ROUTE_BINDING_INVALID,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Edge anti-corruption adapter that fail-closes Inference route binding
/// publication against DomainClaim (and GatewayScope when present).
#[derive(Clone)]
pub struct EdgeInferenceRouteBindingAdmissionAdapter {
    edge: Arc<dyn IEdgeRepository>,
}

impl EdgeInferenceRouteBindingAdmissionAdapter {
    pub fn new(edge: Arc<dyn IEdgeRepository>) -> Self {
        Self { edge }
    }
}

#[async_trait]
impl IInferenceEdgeRouteBindingAdmissionPort for EdgeInferenceRouteBindingAdmissionAdapter {
    async fn admit(
        &self,
        request: InferenceEdgeRouteBindingAdmissionRequest,
    ) -> ApplicationResult<()> {
        if let Err(error) = request.binding.validate() {
            return Err(binding_invalid(error));
        }
        if request.binding.path_prefix.is_empty() {
            return Err(binding_invalid(
                "inference edge binding path_prefix must be non-empty",
            ));
        }
        if request.binding.binding_generation == 0 {
            return Err(binding_invalid(
                "inference edge binding_generation must be greater than 0",
            ));
        }

        let hostname = match RouteHostname::parse(request.binding.hostname.as_str()) {
            Ok(value) => value,
            Err(error) => return Err(binding_invalid(error)),
        };

        let claim = match self
            .edge
            .find_domain_claim(
                request.organization_id,
                request.binding.domain_claim_id,
            )
            .await
        {
            Ok(claim) => claim,
            Err(RepositoryError::NotFound) => {
                return Err(binding_invalid("domain claim not found for binding"))
            }
            Err(error) => return Err(error.into()),
        };

        if claim.organization_id != request.organization_id
            || claim.project_id != request.project_id
            || claim.environment_id != request.environment_id
        {
            return Err(binding_invalid(
                "domain claim does not belong to this organization, project, and environment",
            ));
        }
        if claim.state != DomainClaimState::Verified {
            return Err(binding_invalid("domain claim is not verified"));
        }
        if !claim.covers(&hostname) {
            return Err(binding_invalid(
                "verified domain claim does not cover binding hostname",
            ));
        }

        match self
            .edge
            .find_gateway_scope(
                request.organization_id,
                request.binding.gateway_scope_id,
            )
            .await
        {
            Ok(scope) => {
                if scope.organization_id != request.organization_id
                    || scope.project_id != request.project_id
                    || scope.environment_id != request.environment_id
                {
                    return Err(binding_invalid(
                        "gateway scope does not belong to this organization, project, and environment",
                    ));
                }
            }
            Err(RepositoryError::NotFound) => {
                // Scope membership is checked only when Edge has the scope.
                // A missing scope does not invent membership; publication still
                // requires a non-nil gateway_scope_id on the binding reference.
            }
            Err(error) => return Err(error.into()),
        }

        Ok(())
    }
}

fn binding_invalid(detail: impl Into<String>) -> ApplicationError {
    ApplicationError::Invalid(format!(
        "{EDGE_ROUTE_BINDING_INVALID}: {}",
        detail.into()
    ))
}
