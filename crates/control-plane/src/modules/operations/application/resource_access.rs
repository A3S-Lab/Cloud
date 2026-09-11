use crate::modules::operations::domain::value_objects::OperationSubject;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use async_trait::async_trait;
use std::collections::BTreeSet;

/// One Operations visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no ownership meaning
/// for Operation subjects and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OperationAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

/// Operations-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value, while Operations
/// filters polymorphic subjects through owner ports without importing Identity
/// policy vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<OperationAccessScope>,
}

impl OperationAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(
        granted_scopes: impl IntoIterator<Item = OperationAccessScope>,
    ) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = OperationAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }
}

/// Application port for resolving a polymorphic Operation subject through its owning context.
///
/// Implementations must use only the subject kind and ID. Operation input is workflow payload,
/// not an ownership authority, and must never be used to infer a grant scope.
#[async_trait]
pub(crate) trait IOperationResourceAccess: Send + Sync {
    async fn subject_is_visible(
        &self,
        organization_id: OrganizationId,
        subject: &OperationSubject,
        access: &OperationAccess,
    ) -> ApplicationResult<bool>;
}
