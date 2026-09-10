//! Inference-owned environment model route catalog head.

use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, InferenceRouteId, OrganizationId, ProjectId,
};
use a3s_cloud_contracts::{
    InferenceGrantAclProjection, InferenceModelAclProjection, InferenceRouteAclProjection,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

/// Durable Inference route catalog head with an immutable access-policy revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceRoute {
    pub id: InferenceRouteId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    router: String,
    policy_revision: u64,
    models: Vec<InferenceModelAclProjection>,
    grants: Vec<InferenceGrantAclProjection>,
    binding: EdgeRouteBindingRef,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    retired_at: Option<DateTime<Utc>>,
}

impl InferenceRoute {
    #[allow(clippy::too_many_arguments)]
    pub fn publish(
        id: InferenceRouteId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        router: impl Into<String>,
        models: Vec<InferenceModelAclProjection>,
        grants: Vec<InferenceGrantAclProjection>,
        binding: EdgeRouteBindingRef,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        Self::restore(
            id,
            organization_id,
            project_id,
            environment_id,
            router,
            1,
            models,
            grants,
            binding,
            1,
            created_at,
            created_at,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: InferenceRouteId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        router: impl Into<String>,
        policy_revision: u64,
        models: Vec<InferenceModelAclProjection>,
        grants: Vec<InferenceGrantAclProjection>,
        binding: EdgeRouteBindingRef,
        aggregate_version: u64,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        retired_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let route = Self {
            id,
            organization_id,
            project_id,
            environment_id,
            router: router.into(),
            policy_revision,
            models,
            grants,
            binding,
            aggregate_version,
            created_at: canonical_timestamp(created_at),
            updated_at: canonical_timestamp(updated_at),
            retired_at: retired_at.map(canonical_timestamp),
        };
        route.validate()?;
        Ok(route)
    }

    pub fn revise(
        &mut self,
        router: impl Into<String>,
        models: Vec<InferenceModelAclProjection>,
        grants: Vec<InferenceGrantAclProjection>,
        binding: EdgeRouteBindingRef,
        revised_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.retired_at.is_some() {
            return Err("retired inference route cannot be revised".into());
        }
        let revised_at = canonical_timestamp(revised_at);
        if revised_at < self.updated_at {
            return Err("inference route revision time regressed".into());
        }
        let policy_revision = self
            .policy_revision
            .checked_add(1)
            .filter(|revision| *revision <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "inference route policy_revision is exhausted".to_owned())?;
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .filter(|version| *version <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "inference route aggregate version is exhausted".to_owned())?;
        let candidate = Self::restore(
            self.id,
            self.organization_id,
            self.project_id,
            self.environment_id,
            router,
            policy_revision,
            models,
            grants,
            binding,
            aggregate_version,
            self.created_at,
            revised_at,
            None,
        )?;
        *self = candidate;
        Ok(())
    }

    pub fn retire(&mut self, retired_at: DateTime<Utc>) -> Result<bool, String> {
        if self.retired_at.is_some() {
            return Ok(false);
        }
        let retired_at = canonical_timestamp(retired_at);
        if retired_at < self.updated_at {
            return Err("inference route retirement time regressed".into());
        }
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .filter(|version| *version <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "inference route aggregate version is exhausted".to_owned())?;
        self.aggregate_version = aggregate_version;
        self.updated_at = retired_at;
        self.retired_at = Some(retired_at);
        self.validate()?;
        Ok(true)
    }

    pub fn is_retired(&self) -> bool {
        self.retired_at.is_some()
    }

    pub fn gateway_projection(&self) -> Result<InferenceRouteAclProjection, String> {
        if self.retired_at.is_some() {
            return Err("retired inference route is not projected into Gateway ACL".into());
        }
        let projection = InferenceRouteAclProjection {
            route_id: self.id.as_uuid(),
            router: self.router.clone(),
            environment_id: self.environment_id.as_uuid(),
            policy_revision: self.policy_revision,
            models: self.models.clone(),
            grants: self.grants.clone(),
        };
        projection.validate()?;
        Ok(projection)
    }

    pub fn router(&self) -> &str {
        &self.router
    }

    pub const fn policy_revision(&self) -> u64 {
        self.policy_revision
    }

    pub fn models(&self) -> &[InferenceModelAclProjection] {
        &self.models
    }

    pub fn grants(&self) -> &[InferenceGrantAclProjection] {
        &self.grants
    }

    pub const fn binding(&self) -> &EdgeRouteBindingRef {
        &self.binding
    }

    pub const fn aggregate_version(&self) -> u64 {
        self.aggregate_version
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub const fn retired_at(&self) -> Option<DateTime<Utc>> {
        self.retired_at
    }

    pub fn validate_transition_from(
        &self,
        existing: &Self,
        expected_aggregate_version: u64,
    ) -> Result<(), String> {
        if existing.aggregate_version != expected_aggregate_version
            || self.aggregate_version != expected_aggregate_version.checked_add(1).unwrap_or(0)
            || self.id != existing.id
            || self.organization_id != existing.organization_id
            || self.project_id != existing.project_id
            || self.environment_id != existing.environment_id
            || self.created_at != existing.created_at
            || self.updated_at < existing.updated_at
            || existing.retired_at.is_some()
        {
            return Err("inference route optimistic transition is invalid".into());
        }
        let revised = self.policy_revision == existing.policy_revision.checked_add(1).unwrap_or(0)
            && self.retired_at.is_none();
        let retired = self.policy_revision == existing.policy_revision
            && self.retired_at == Some(self.updated_at)
            && self.router == existing.router
            && self.models == existing.models
            && self.grants == existing.grants
            && self.binding == existing.binding;
        if !revised && !retired {
            return Err("inference route update must be one revision or retirement".into());
        }
        self.validate()
    }

    fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.policy_revision == 0
            || self.policy_revision > MAX_SAFE_ACL_INTEGER
            || self.aggregate_version == 0
            || self.aggregate_version > MAX_SAFE_ACL_INTEGER
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
        {
            return Err("inference route identity, version, or timestamps are invalid".into());
        }
        if self.retired_at.is_some_and(|retired_at| {
            retired_at != canonical_timestamp(retired_at)
                || retired_at < self.created_at
                || retired_at != self.updated_at
        }) {
            return Err("inference route retirement timestamp is invalid".into());
        }
        self.binding.validate()?;
        let projection = InferenceRouteAclProjection {
            route_id: self.id.as_uuid(),
            router: self.router.clone(),
            environment_id: self.environment_id.as_uuid(),
            policy_revision: self.policy_revision,
            models: self.models.clone(),
            grants: self.grants.clone(),
        };
        projection.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{DomainClaimId, GatewayScopeId};
    use a3s_cloud_contracts::{
        InferenceEndpointAcl, InferenceLimitsAclProjection, InferenceTargetAclProjection,
    };
    use chrono::Duration;
    use uuid::Uuid;

    fn binding() -> EdgeRouteBindingRef {
        EdgeRouteBindingRef::new(
            DomainClaimId::new(),
            GatewayScopeId::new(),
            "api.example.com",
            "/v1",
            1,
        )
        .unwrap()
    }

    fn model() -> InferenceModelAclProjection {
        InferenceModelAclProjection {
            alias: "chat-model".into(),
            model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
            targets: vec![InferenceTargetAclProjection {
                target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
                service: "model-service".into(),
                upstream_model: "internal/model-v1".into(),
                priority: 0,
                weight: 100,
            }],
        }
    }

    fn grant() -> InferenceGrantAclProjection {
        InferenceGrantAclProjection {
            credential_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
            credential_generation: 3,
            models: vec!["chat-model".into()],
            endpoints: vec![InferenceEndpointAcl::Models, InferenceEndpointAcl::ChatCompletions],
            limits: InferenceLimitsAclProjection {
                max_concurrent_requests: 2,
                requests_per_minute: 60,
                request_burst: 2,
                tokens_per_minute: 10_000,
            },
        }
    }

    fn route() -> InferenceRoute {
        InferenceRoute::publish(
            InferenceRouteId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "inference",
            vec![model()],
            vec![grant()],
            binding(),
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn publish_projects_models_and_grants_without_workers() {
        let route = route();
        let projection = route.gateway_projection().unwrap();
        assert_eq!(projection.models.len(), 1);
        assert_eq!(projection.grants.len(), 1);
        assert_eq!(projection.policy_revision, 1);
        let rendered = a3s_cloud_contracts::render_inference_route_acl_blocks(&[projection]).unwrap();
        assert!(rendered.contains("models \"chat-model\""));
        assert!(!rendered.contains("workers "));
    }

    #[test]
    fn revise_bumps_policy_revision_and_retire_blocks_projection() {
        let mut route = route();
        let revised_at = route.updated_at() + Duration::seconds(1);
        route
            .revise(
                "inference",
                vec![model()],
                vec![grant()],
                binding(),
                revised_at,
            )
            .unwrap();
        assert_eq!(route.policy_revision(), 2);
        assert!(route.retire(revised_at + Duration::seconds(1)).unwrap());
        assert!(route
            .gateway_projection()
            .unwrap_err()
            .contains("retired"));
    }

    #[test]
    fn rejects_unknown_grant_alias_and_zero_weight_target() {
        let mut bad_grant = grant();
        bad_grant.models = vec!["missing".into()];
        let err = InferenceRoute::publish(
            InferenceRouteId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "inference",
            vec![model()],
            vec![bad_grant],
            binding(),
            Utc::now(),
        )
        .unwrap_err();
        assert!(err.contains("unknown model alias"));

        let mut bad_model = model();
        bad_model.targets[0].weight = 0;
        let err = InferenceRoute::publish(
            InferenceRouteId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "inference",
            vec![bad_model],
            vec![grant()],
            binding(),
            Utc::now(),
        )
        .unwrap_err();
        assert!(err.contains("weight"));
    }
}
