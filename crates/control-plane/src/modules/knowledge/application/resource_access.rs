use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::ProjectId;
use std::collections::BTreeSet;

/// Knowledge-owned projection of an already-authorized request.
///
/// This value never grants Identity authority. Presentation may only translate
/// an Identity decision into organization-wide visibility or exact project
/// identifiers; Knowledge then narrows every use case against that projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeAccess {
    organization_wide: bool,
    project_ids: BTreeSet<ProjectId>,
}

impl KnowledgeAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            project_ids: BTreeSet::new(),
        }
    }

    pub fn restricted_projects(project_ids: impl IntoIterator<Item = ProjectId>) -> Self {
        Self {
            organization_wide: false,
            project_ids: project_ids.into_iter().collect(),
        }
    }

    pub fn project_is_visible(&self, project_id: ProjectId) -> bool {
        self.organization_wide || self.project_ids.contains(&project_id)
    }
}

pub(super) fn project(project_id: ProjectId, access: &KnowledgeAccess) -> ApplicationResult<()> {
    if access.project_is_visible(project_id) {
        return Ok(());
    }
    Err(ApplicationError::NotFound(
        "Knowledge project not found".into(),
    ))
}

pub(super) fn knowledge_not_found() -> ApplicationError {
    ApplicationError::NotFound("Knowledge resource not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_access_is_exact_project_only() {
        let granted = ProjectId::new();
        let access = KnowledgeAccess::restricted_projects([granted, granted]);
        assert!(access.project_is_visible(granted));
        assert!(!access.project_is_visible(ProjectId::new()));
    }
}
