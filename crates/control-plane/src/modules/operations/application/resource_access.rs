use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::operations::domain::value_objects::OperationSubject;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use async_trait::async_trait;

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
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<bool>;
}
