use crate::modules::assets::domain::{Asset, AssetWrite};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetResponse {
    pub organization_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub state: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed: Option<bool>,
}

impl From<Asset> for AssetResponse {
    fn from(asset: Asset) -> Self {
        Self {
            organization_id: asset.organization_id.as_uuid(),
            id: asset.id.as_uuid(),
            name: asset.name.as_str().to_owned(),
            kind: asset.kind.as_str().to_owned(),
            state: asset.state.as_str().to_owned(),
            aggregate_version: asset.aggregate_version,
            created_at: asset.created_at,
            updated_at: asset.updated_at,
            archived_at: asset.archived_at,
            replayed: None,
        }
    }
}

impl From<AssetWrite> for AssetResponse {
    fn from(write: AssetWrite) -> Self {
        let mut response = Self::from(write.asset);
        response.replayed = Some(write.replayed);
        response
    }
}
