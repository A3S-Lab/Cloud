use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::shared_kernel::domain::{DomainClaimId, GatewayScopeId};
use a3s_cloud_contracts::{
    InferenceEndpointAcl, InferenceGrantAclProjection, InferenceLimitsAclProjection,
    InferenceModelAclProjection, InferenceTargetAclProjection,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishInferenceRouteRequest {
    pub router: String,
    pub models: Vec<PublishInferenceModelRequest>,
    pub grants: Vec<PublishInferenceGrantRequest>,
    pub binding: PublishEdgeRouteBindingRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishInferenceModelRequest {
    pub alias: String,
    pub model_id: Uuid,
    pub targets: Vec<PublishInferenceTargetRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishInferenceTargetRequest {
    pub target_id: Uuid,
    pub service: String,
    pub upstream_model: String,
    pub priority: u32,
    pub weight: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishInferenceGrantRequest {
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub models: Vec<String>,
    pub endpoints: Vec<InferenceEndpointAcl>,
    pub limits: PublishInferenceLimitsRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishInferenceLimitsRequest {
    pub max_concurrent_requests: u64,
    pub requests_per_minute: u64,
    pub request_burst: u64,
    pub tokens_per_minute: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishEdgeRouteBindingRequest {
    pub domain_claim_id: Uuid,
    pub gateway_scope_id: Uuid,
    pub hostname: String,
    pub path_prefix: String,
    pub binding_generation: u64,
}

impl PublishInferenceRouteRequest {
    pub fn into_parts(
        self,
    ) -> Result<
        (
            String,
            Vec<InferenceModelAclProjection>,
            Vec<InferenceGrantAclProjection>,
            EdgeRouteBindingRef,
        ),
        String,
    > {
        let models = self
            .models
            .into_iter()
            .map(|model| InferenceModelAclProjection {
                alias: model.alias,
                model_id: model.model_id,
                targets: model
                    .targets
                    .into_iter()
                    .map(|target| InferenceTargetAclProjection {
                        target_id: target.target_id,
                        service: target.service,
                        upstream_model: target.upstream_model,
                        priority: target.priority,
                        weight: target.weight,
                    })
                    .collect(),
            })
            .collect();
        let grants = self
            .grants
            .into_iter()
            .map(|grant| InferenceGrantAclProjection {
                credential_id: grant.credential_id,
                credential_generation: grant.credential_generation,
                models: grant.models,
                endpoints: grant.endpoints,
                limits: InferenceLimitsAclProjection {
                    max_concurrent_requests: grant.limits.max_concurrent_requests,
                    requests_per_minute: grant.limits.requests_per_minute,
                    request_burst: grant.limits.request_burst,
                    tokens_per_minute: grant.limits.tokens_per_minute,
                },
            })
            .collect();
        let binding = EdgeRouteBindingRef::new(
            DomainClaimId::from_uuid(self.binding.domain_claim_id),
            GatewayScopeId::from_uuid(self.binding.gateway_scope_id),
            self.binding.hostname,
            self.binding.path_prefix,
            self.binding.binding_generation,
        )?;
        Ok((self.router, models, grants, binding))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseInferenceRouteRequest {
    pub expected_aggregate_version: u64,
    #[serde(flatten)]
    pub policy: PublishInferenceRouteRequest,
}

impl ReviseInferenceRouteRequest {
    pub fn into_parts(
        self,
    ) -> Result<
        (
            u64,
            String,
            Vec<InferenceModelAclProjection>,
            Vec<InferenceGrantAclProjection>,
            EdgeRouteBindingRef,
        ),
        String,
    > {
        let (router, models, grants, binding) = self.policy.into_parts()?;
        Ok((
            self.expected_aggregate_version,
            router,
            models,
            grants,
            binding,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetireInferenceRouteRequest {
    pub expected_aggregate_version: u64,
}
