use crate::modules::plugins::domain::entities::PluginPlanProjection;
use crate::modules::plugins::domain::repositories::IPluginPlanProjectionRepository;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PluginAssignmentId, PluginPlanProjectionId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryPluginPlanProjectionRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    projections: BTreeMap<(OrganizationId, PluginPlanProjectionId), PluginPlanProjection>,
    digests: BTreeMap<(OrganizationId, String), PluginPlanProjectionId>,
}

impl InMemoryPluginPlanProjectionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IPluginPlanProjectionRepository for InMemoryPluginPlanProjectionRepository {
    async fn create(
        &self,
        projection: PluginPlanProjection,
    ) -> Result<PluginPlanProjection, RepositoryError> {
        projection.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let digest_key = (
            projection.organization_id,
            projection.plan_digest.as_str().to_owned(),
        );
        if state.digests.contains_key(&digest_key) {
            return Err(RepositoryError::Conflict(
                "plugin plan projection already exists for this plan digest".into(),
            ));
        }
        state.digests.insert(digest_key, projection.id);
        state.projections.insert(
            (projection.organization_id, projection.id),
            projection.clone(),
        );
        Ok(projection)
    }

    async fn update(
        &self,
        projection: PluginPlanProjection,
        expected_confirmation_digest: Option<&Sha256Digest>,
    ) -> Result<PluginPlanProjection, RepositoryError> {
        projection.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (projection.organization_id, projection.id);
        let Some(existing) = state.projections.get(&key) else {
            return Err(RepositoryError::NotFound);
        };
        if existing.confirmation_digest.as_ref() != expected_confirmation_digest {
            return Err(RepositoryError::Conflict(
                "plugin plan projection confirmation changed concurrently".into(),
            ));
        }
        state.projections.insert(key, projection.clone());
        Ok(projection)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        projection_id: PluginPlanProjectionId,
    ) -> Result<Option<PluginPlanProjection>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .projections
            .get(&(organization_id, projection_id))
            .cloned())
    }

    async fn find_by_plan_digest(
        &self,
        organization_id: OrganizationId,
        plan_digest: &Sha256Digest,
    ) -> Result<Option<PluginPlanProjection>, RepositoryError> {
        let state = self.state.read().await;
        let Some(id) = state
            .digests
            .get(&(organization_id, plan_digest.as_str().to_owned()))
            .copied()
        else {
            return Ok(None);
        };
        Ok(state.projections.get(&(organization_id, id)).cloned())
    }

    async fn list_for_assignment(
        &self,
        organization_id: OrganizationId,
        assignment_id: PluginAssignmentId,
    ) -> Result<Vec<PluginPlanProjection>, RepositoryError> {
        let mut rows = self
            .state
            .read()
            .await
            .projections
            .values()
            .filter(|projection| {
                projection.organization_id == organization_id
                    && projection.assignment_id == assignment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|projection| (projection.created_at, projection.id));
        Ok(rows)
    }
}
