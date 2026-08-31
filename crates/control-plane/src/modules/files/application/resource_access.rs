use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::ProjectId;
use std::collections::BTreeSet;

/// Files-owned projection of an already-authorized request.
///
/// This value never grants Identity authority. The root Presentation ACL may
/// only translate an Identity decision into organization-wide visibility or
/// exact project identifiers; Files then narrows every use case against that
/// immutable projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFileAccess {
    organization_wide: bool,
    project_ids: BTreeSet<ProjectId>,
}

impl UserFileAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            project_ids: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted_projects(project_ids: impl IntoIterator<Item = ProjectId>) -> Self {
        Self {
            organization_wide: false,
            project_ids: project_ids.into_iter().collect(),
        }
    }

    pub(crate) fn project_is_visible(&self, project_id: ProjectId) -> bool {
        self.organization_wide || self.project_ids.contains(&project_id)
    }

    pub(crate) const fn organization_quota_is_visible(&self) -> bool {
        self.organization_wide
    }
}

pub(super) fn project(project_id: ProjectId, access: &UserFileAccess) -> ApplicationResult<()> {
    if access.project_is_visible(project_id) {
        return Ok(());
    }
    Err(ApplicationError::NotFound(
        "UserFile project not found".into(),
    ))
}

pub(super) fn organization_quota(access: &UserFileAccess) -> ApplicationResult<()> {
    if access.organization_quota_is_visible() {
        return Ok(());
    }
    Err(ApplicationError::NotFound(
        "UserFile organization quota not found".into(),
    ))
}

pub(super) fn user_file_not_found() -> ApplicationError {
    ApplicationError::NotFound("UserFile not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_access_is_exact_project_only_and_never_reveals_quota() {
        let granted = ProjectId::new();
        let access = UserFileAccess::restricted_projects([granted, granted]);

        assert!(access.project_is_visible(granted));
        assert!(!access.project_is_visible(ProjectId::new()));
        assert!(!access.organization_quota_is_visible());
    }

    #[test]
    fn organization_access_explicitly_reveals_projects_and_quota() {
        let access = UserFileAccess::organization_wide();

        assert!(access.project_is_visible(ProjectId::new()));
        assert!(access.organization_quota_is_visible());
    }
}
