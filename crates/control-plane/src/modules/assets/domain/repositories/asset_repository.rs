use crate::modules::assets::domain::{
    Asset, AssetArchived, AssetCreated, AssetKind, AssetRelease, AssetReleaseDrafted,
    AssetReleasePublished, AssetReleaseState, AssetReleaseYanked, AssetState,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, IdempotencyRequest, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetWriteReference {
    pub asset_id: AssetId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReleaseWriteReference {
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetWrite {
    pub asset: Asset,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetReleaseWrite {
    pub asset: Asset,
    pub release: AssetRelease,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct CreateAssetWrite {
    pub asset: Asset,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct TransitionAssetWrite {
    pub asset: Asset,
    pub expected_aggregate_version: u64,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct CreateAssetReleaseWrite {
    pub release: AssetRelease,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct TransitionAssetReleaseWrite {
    pub release: AssetRelease,
    pub expected_aggregate_version: u64,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl CreateAssetWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.asset.validate()?;
        if self.asset.state != AssetState::Active
            || self.asset.aggregate_version != 1
            || self.asset.created_at != self.asset.updated_at
            || self.asset.archived_at.is_some()
        {
            return Err("new Asset is not at its initial state".into());
        }
        validate_event(&self.event, &self.asset, "asset.asset.created")
    }
}

impl TransitionAssetWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.asset.validate()?;
        if self.asset.state != AssetState::Archived {
            return Err("Asset transition must archive the aggregate".into());
        }
        validate_event(&self.event, &self.asset, "asset.asset.archived")
    }

    pub fn validate_against(&self, existing: &Asset) -> Result<(), String> {
        existing.validate()?;
        if existing.aggregate_version != self.expected_aggregate_version
            || self.expected_aggregate_version.checked_add(1) != Some(self.asset.aggregate_version)
            || existing.state != AssetState::Active
            || self.asset.state != AssetState::Archived
            || existing.id != self.asset.id
            || existing.organization_id != self.asset.organization_id
            || existing.name != self.asset.name
            || existing.kind != self.asset.kind
            || existing.created_at != self.asset.created_at
            || self.asset.updated_at < existing.updated_at
        {
            return Err("Asset changed while archiving".into());
        }
        Ok(())
    }
}

impl CreateAssetReleaseWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.release.validate()?;
        if self.release.state != AssetReleaseState::Draft
            || self.release.aggregate_version != 1
            || self.release.created_at != self.release.updated_at
        {
            return Err("new Asset release is not at its initial draft state".into());
        }
        validate_release_event(&self.event, &self.release, "asset.release.drafted")
    }
}

impl TransitionAssetReleaseWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.release.validate()?;
        let event_key = match self.release.state {
            AssetReleaseState::Draft => {
                return Err("Asset release transition cannot retain draft state".into())
            }
            AssetReleaseState::Published => "asset.release.published",
            AssetReleaseState::Yanked => "asset.release.yanked",
        };
        validate_release_event(&self.event, &self.release, event_key)
    }

    pub fn validate_against(&self, existing: &AssetRelease, asset: &Asset) -> Result<(), String> {
        existing.validate_for(asset)?;
        self.release.validate_for(asset)?;
        if existing.aggregate_version != self.expected_aggregate_version
            || self.expected_aggregate_version.checked_add(1)
                != Some(self.release.aggregate_version)
            || existing.id != self.release.id
            || existing.organization_id != self.release.organization_id
            || existing.asset_id != self.release.asset_id
            || existing.version != self.release.version
            || existing.commit_sha != self.release.commit_sha
            || existing.manifest_digest != self.release.manifest_digest
            || existing.created_at != self.release.created_at
            || self.release.updated_at < existing.updated_at
        {
            return Err("Asset release changed during its transition".into());
        }
        match (existing.state, self.release.state) {
            (AssetReleaseState::Draft, AssetReleaseState::Published)
                if asset.state == AssetState::Active
                    && existing.artifact.is_none()
                    && existing.provenance.is_none()
                    && matches!(
                        (asset.kind, self.release.provenance.is_some()),
                        (AssetKind::Skill, false) | (AssetKind::Agent | AssetKind::Mcp, true)
                    ) =>
            {
                Ok(())
            }
            (AssetReleaseState::Published, AssetReleaseState::Yanked)
                if existing.artifact == self.release.artifact
                    && existing.provenance == self.release.provenance =>
            {
                Ok(())
            }
            _ => Err("Asset release transition is not allowed".into()),
        }
    }
}

#[async_trait]
pub trait IAssetRepository: Send + Sync {
    async fn create_asset(&self, bundle: CreateAssetWrite) -> Result<AssetWrite, RepositoryError>;

    async fn transition_asset(
        &self,
        bundle: TransitionAssetWrite,
    ) -> Result<AssetWrite, RepositoryError>;

    async fn find_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Option<Asset>, RepositoryError>;

    async fn list_assets(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<Asset>, RepositoryError>;

    async fn create_release(
        &self,
        bundle: CreateAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError>;

    async fn transition_release(
        &self,
        bundle: TransitionAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError>;

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<AssetRelease>, RepositoryError>;

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Vec<AssetRelease>, RepositoryError>;
}

fn validate_event(
    event: &DomainEventEnvelope,
    asset: &Asset,
    event_key: &str,
) -> Result<(), String> {
    if !event_metadata_matches(
        event,
        event_key,
        1,
        asset.organization_id.as_uuid(),
        asset.id.as_uuid(),
        asset.aggregate_version,
        asset.updated_at,
    ) {
        return Err("Asset write and domain event are inconsistent".into());
    }
    match event_key {
        "asset.asset.created" => {
            let payload: AssetCreated = event_payload(event, "Asset created")?;
            if payload.organization_id == asset.organization_id.as_uuid()
                && payload.asset_id == asset.id.as_uuid()
                && payload.name == asset.name.as_str()
                && payload.kind == asset.kind.as_str()
            {
                Ok(())
            } else {
                Err("Asset created event payload is inconsistent".into())
            }
        }
        "asset.asset.archived" => {
            let payload: AssetArchived = event_payload(event, "Asset archived")?;
            if payload.organization_id == asset.organization_id.as_uuid()
                && payload.asset_id == asset.id.as_uuid()
            {
                Ok(())
            } else {
                Err("Asset archived event payload is inconsistent".into())
            }
        }
        _ => Err("unsupported Asset event key".into()),
    }
}

fn validate_release_event(
    event: &DomainEventEnvelope,
    release: &AssetRelease,
    event_key: &str,
) -> Result<(), String> {
    let schema_version = if event_key == "asset.release.published" && release.provenance.is_some() {
        2
    } else {
        1
    };
    if !event_metadata_matches(
        event,
        event_key,
        schema_version,
        release.organization_id.as_uuid(),
        release.id.as_uuid(),
        release.aggregate_version,
        release.updated_at,
    ) {
        return Err("Asset release write and domain event are inconsistent".into());
    }
    match event_key {
        "asset.release.drafted" => {
            let payload: AssetReleaseDrafted = event_payload(event, "Asset release drafted")?;
            if payload.organization_id == release.organization_id.as_uuid()
                && payload.asset_id == release.asset_id.as_uuid()
                && payload.asset_release_id == release.id.as_uuid()
                && payload.version == release.version.as_str()
                && payload.commit_sha == release.commit_sha.as_str()
                && payload.manifest_digest == release.manifest_digest.as_str()
            {
                Ok(())
            } else {
                Err("Asset release drafted event payload is inconsistent".into())
            }
        }
        "asset.release.published" => {
            let payload: AssetReleasePublished = event_payload(event, "Asset release published")?;
            let artifact = release
                .artifact
                .as_ref()
                .ok_or_else(|| "published Asset release has no artifact".to_owned())?;
            if payload.organization_id == release.organization_id.as_uuid()
                && payload.asset_id == release.asset_id.as_uuid()
                && payload.asset_release_id == release.id.as_uuid()
                && payload.version == release.version.as_str()
                && payload.artifact_kind == artifact.kind().as_str()
                && payload.artifact_digest == artifact.digest().as_str()
                && payload.build_run_id
                    == release
                        .provenance
                        .as_ref()
                        .map(|provenance| provenance.build_run_id().as_uuid())
                && payload.provenance_digest.as_deref()
                    == release
                        .provenance
                        .as_ref()
                        .map(|provenance| provenance.provenance_digest().as_str())
            {
                Ok(())
            } else {
                Err("Asset release published event payload is inconsistent".into())
            }
        }
        "asset.release.yanked" => {
            let payload: AssetReleaseYanked = event_payload(event, "Asset release yanked")?;
            if payload.organization_id == release.organization_id.as_uuid()
                && payload.asset_id == release.asset_id.as_uuid()
                && payload.asset_release_id == release.id.as_uuid()
                && payload.version == release.version.as_str()
            {
                Ok(())
            } else {
                Err("Asset release yanked event payload is inconsistent".into())
            }
        }
        _ => Err("unsupported Asset release event key".into()),
    }
}

fn event_metadata_matches(
    event: &DomainEventEnvelope,
    event_key: &str,
    schema_version: u32,
    organization_id: uuid::Uuid,
    aggregate_id: uuid::Uuid,
    aggregate_version: u64,
    occurred_at: chrono::DateTime<chrono::Utc>,
) -> bool {
    !event.event_id.is_nil()
        && !event.correlation_id.is_nil()
        && event
            .causation_id
            .is_none_or(|causation_id| !causation_id.is_nil())
        && event.event_key == event_key
        && event.schema_version == schema_version
        && event.organization_id == organization_id
        && event.aggregate_id == aggregate_id
        && event.aggregate_version == aggregate_version
        && event.occurred_at == occurred_at
}

fn event_payload<T>(event: &DomainEventEnvelope, label: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("{label} event payload is invalid: {error}"))
}
