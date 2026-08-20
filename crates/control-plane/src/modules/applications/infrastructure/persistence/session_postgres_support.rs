use crate::infrastructure::{
    is_foreign_key_violation, is_unique_violation, PostgresPersistenceError,
};
use crate::modules::applications::domain::{
    ApplicationWorkflowEffect, OpenApplicationSessionWrite,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationSessionId, OrganizationId, RepositoryError,
};
use a3s_orm::PostgresTransaction;

use super::session_postgres_loads::load_effect_claim;

pub(super) async fn lock_open_identity(
    transaction: &PostgresTransaction,
    write: &OpenApplicationSessionWrite,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock(
            "a3s.cloud.application-end-user",
            &format!(
                "{}:{}:{}:{}",
                write.end_user.organization_id,
                write.end_user.project_id,
                write.end_user.application_id,
                write.end_user.id
            ),
        )
        .await?;
    transaction
        .advisory_xact_lock(
            "a3s.cloud.application-session",
            &format!(
                "{}:{}:{}:{}",
                write.session.organization_id,
                write.session.project_id,
                write.session.application_id,
                write.session.id
            ),
        )
        .await?;
    Ok(())
}

pub(super) async fn ensure_unclaimed_effect(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
    effect: &ApplicationWorkflowEffect,
) -> Result<(), PostgresPersistenceError> {
    if load_effect_claim(
        transaction,
        organization_id,
        application_id,
        session_id,
        effect,
    )
    .await?
    .is_some()
    {
        return Err(RepositoryError::Conflict(
            "Workflow effect is already owned by another Application semantic write".into(),
        )
        .into());
    }
    Ok(())
}

pub(super) fn write_error(
    error: PostgresPersistenceError,
    conflict: &'static str,
) -> PostgresPersistenceError {
    if is_unique_violation(&error) {
        RepositoryError::Conflict(conflict.into()).into()
    } else if is_foreign_key_violation(&error) {
        RepositoryError::NotFound.into()
    } else {
        error
    }
}
