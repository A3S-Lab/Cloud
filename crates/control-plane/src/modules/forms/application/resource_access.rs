use crate::modules::forms::domain::{FormDraft, IFormRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{FormId, OrganizationId, ProjectId, RepositoryError};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One project authority projected into Forms from an authenticated request.
///
/// Forms are project-owned aggregates. Environment and Node grants have no
/// authority meaning here and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FormAccessScope {
    Project { project_id: ProjectId },
}

impl FormAccessScope {
    fn allows(self, project_id: ProjectId) -> bool {
        match self {
            Self::Project {
                project_id: granted,
            } => granted == project_id,
        }
    }
}

/// Forms-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Inbound
/// adapters narrow that decision into this immutable value, while Forms owns
/// resource-to-project resolution and conceals missing and denied identifiers
/// through the same not-found contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<FormAccessScope>,
}

impl FormAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(granted_scopes: impl IntoIterator<Item = FormAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) fn project_is_visible(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.allows(project_id))
    }
}

/// Resolves indirect Form identifiers through the Forms authority before authorization.
///
/// Forms owns the canonical draft-to-project relationship. Releases inherit that relationship
/// from their draft, while Form submissions remain owned by their Workflow/HumanTask aggregate.
/// Missing and denied draft identifiers intentionally share the same not-found contract.
#[derive(Clone)]
pub(crate) struct FormResourceResolver {
    forms: Arc<dyn IFormRepository>,
}

impl FormResourceResolver {
    pub fn new(forms: Arc<dyn IFormRepository>) -> Self {
        Self { forms }
    }

    pub async fn draft(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
        access: &FormAccess,
    ) -> ApplicationResult<FormDraft> {
        let draft = match self.forms.find_draft(organization_id, form_id).await {
            Ok(Some(draft)) => draft,
            Ok(None) => return Err(form_not_found()),
            Err(RepositoryError::NotFound) => return Err(form_not_found()),
            Err(error) => return Err(error.into()),
        };
        if !access.project_is_visible(draft.project_id) {
            return Err(form_not_found());
        }
        Ok(draft)
    }
}

fn form_not_found() -> ApplicationError {
    ApplicationError::NotFound("Form not found".into())
}

#[cfg(test)]
impl FormAccess {
    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = FormAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_visibility_is_exact_and_canonicalized() {
        let project_id = ProjectId::new();
        let scope = FormAccessScope::Project { project_id };
        let access = FormAccess::restricted([scope, scope]);

        assert_eq!(access.granted_scopes().collect::<Vec<_>>(), [scope]);
        assert!(access.project_is_visible(project_id));
        assert!(!access.project_is_visible(ProjectId::new()));
    }

    #[test]
    fn organization_wide_access_sees_every_project() {
        assert!(FormAccess::organization_wide().project_is_visible(ProjectId::new()));
    }
}
