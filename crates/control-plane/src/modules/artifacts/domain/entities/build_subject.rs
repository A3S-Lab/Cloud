use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, EnvironmentId, ProjectId, SourceRevisionId,
};
use serde::{Deserialize, Serialize};

/// The immutable business identity whose source is built by the canonical
/// Cloud build workflow.
///
/// The untagged representation deliberately preserves the existing external
/// source BuildRun JSON fields while admitting the organization-scoped hosted
/// Asset release subject without synthetic Project, Environment, or Source
/// records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BuildSubject {
    ExternalSourceRevision {
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
    },
    AssetRelease {
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    },
}

impl BuildSubject {
    pub const fn external_source_revision(
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
    ) -> Self {
        Self::ExternalSourceRevision {
            project_id,
            environment_id,
            source_revision_id,
        }
    }

    pub const fn asset_release(asset_id: AssetId, asset_release_id: AssetReleaseId) -> Self {
        Self::AssetRelease {
            asset_id,
            asset_release_id,
        }
    }

    pub const fn kind(self) -> &'static str {
        match self {
            Self::ExternalSourceRevision { .. } => "external_source_revision",
            Self::AssetRelease { .. } => "asset_release",
        }
    }

    pub const fn project_id(self) -> Option<ProjectId> {
        match self {
            Self::ExternalSourceRevision { project_id, .. } => Some(project_id),
            Self::AssetRelease { .. } => None,
        }
    }

    pub const fn environment_id(self) -> Option<EnvironmentId> {
        match self {
            Self::ExternalSourceRevision { environment_id, .. } => Some(environment_id),
            Self::AssetRelease { .. } => None,
        }
    }

    pub const fn source_revision_id(self) -> Option<SourceRevisionId> {
        match self {
            Self::ExternalSourceRevision {
                source_revision_id, ..
            } => Some(source_revision_id),
            Self::AssetRelease { .. } => None,
        }
    }

    pub const fn asset_id(self) -> Option<AssetId> {
        match self {
            Self::AssetRelease { asset_id, .. } => Some(asset_id),
            Self::ExternalSourceRevision { .. } => None,
        }
    }

    pub const fn asset_release_id(self) -> Option<AssetReleaseId> {
        match self {
            Self::AssetRelease {
                asset_release_id, ..
            } => Some(asset_release_id),
            Self::ExternalSourceRevision { .. } => None,
        }
    }

    pub fn validate(self) -> Result<(), String> {
        let valid = match self {
            Self::ExternalSourceRevision {
                project_id,
                environment_id,
                source_revision_id,
            } => {
                !project_id.as_uuid().is_nil()
                    && !environment_id.as_uuid().is_nil()
                    && !source_revision_id.as_uuid().is_nil()
            }
            Self::AssetRelease {
                asset_id,
                asset_release_id,
            } => !asset_id.as_uuid().is_nil() && !asset_release_id.as_uuid().is_nil(),
        };
        valid
            .then_some(())
            .ok_or_else(|| "build subject identity is invalid".to_owned())
    }
}
