use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, OrganizationId, ResourceName,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Agent,
    Mcp,
    Skill,
}

impl AssetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Mcp => "mcp",
            Self::Skill => "skill",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "agent" => Ok(Self::Agent),
            "mcp" => Ok(Self::Mcp),
            "skill" => Ok(Self::Skill),
            _ => Err(format!("unsupported Asset kind {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetState {
    Active,
    Archived,
}

impl AssetState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            _ => Err(format!("unsupported Asset state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub organization_id: OrganizationId,
    pub name: ResourceName,
    pub kind: AssetKind,
    pub state: AssetState,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl Asset {
    pub fn create(
        id: AssetId,
        organization_id: OrganizationId,
        name: ResourceName,
        kind: AssetKind,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let asset = Self {
            id,
            organization_id,
            name,
            kind,
            state: AssetState::Active,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            archived_at: None,
        };
        asset.validate()?;
        Ok(asset)
    }

    pub fn archive(&mut self, archived_at: DateTime<Utc>) -> Result<(), String> {
        self.validate()?;
        if self.state == AssetState::Archived {
            return Ok(());
        }
        let archived_at = canonical_timestamp(archived_at);
        if archived_at < self.updated_at {
            return Err("Asset archive time regressed".into());
        }
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Asset aggregate version overflowed".to_owned())?;
        self.state = AssetState::Archived;
        self.updated_at = archived_at;
        self.archived_at = Some(archived_at);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || ResourceName::parse(self.name.as_str())? != self.name
        {
            return Err("Asset identity, name, version, or timestamps are invalid".into());
        }
        let state_is_valid = match self.state {
            AssetState::Active => self.archived_at.is_none(),
            AssetState::Archived => self.archived_at.is_some_and(|archived_at| {
                archived_at == self.updated_at
                    && archived_at >= self.created_at
                    && archived_at == canonical_timestamp(archived_at)
            }),
        };
        if !state_is_valid {
            return Err("Asset state transition is inconsistent".into());
        }
        Ok(())
    }
}
