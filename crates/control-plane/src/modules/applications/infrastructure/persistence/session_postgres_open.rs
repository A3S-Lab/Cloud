use crate::infrastructure::{transaction_error, PostgresPersistenceError};
use crate::modules::applications::domain::{
    ApplicationInvocation, ApplicationSession, OpenApplicationSessionWrite,
    RequestApplicationInvocationWrite,
};
use crate::modules::shared_kernel::domain::{IdempotentWrite, RepositoryError};
use a3s_orm::PostgresExecutor;

use super::session_postgres_loads::{
    load_end_user, load_message, load_variable, lock_invocation, lock_session,
};
use super::session_postgres_support::{lock_open_identity, write_error};
use super::session_postgres_writes::{
    insert_end_user, insert_invocation, insert_message, insert_session, insert_variable_revision,
    update_session,
};

pub(super) async fn open_session(
    executor: &PostgresExecutor,
    write: OpenApplicationSessionWrite,
) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
    write.validate().map_err(RepositoryError::Storage)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                lock_open_identity(transaction, &write).await?;
                if let Some(current) = lock_session(
                    transaction,
                    write.session.organization_id,
                    write.session.project_id,
                    write.session.application_id,
                    write.session.id,
                )
                .await?
                {
                    let end_user = load_end_user(transaction, &write.end_user)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Application session replay end user is missing".into(),
                            )
                        })?;
                    let variables = load_variable(transaction, &write.initial_variables)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Application session replay variables are missing".into(),
                            )
                        })?;
                    if !write
                        .matches_replay(&current, &end_user, &variables)
                        .map_err(PostgresPersistenceError::Invariant)?
                    {
                        return Err(RepositoryError::Conflict(
                            "Application session identity was reused with different initial state"
                                .into(),
                        )
                        .into());
                    }
                    return Ok(IdempotentWrite {
                        value: current,
                        replayed: true,
                    });
                }

                match load_end_user(transaction, &write.end_user).await? {
                    Some(current) if current != write.end_user => {
                        return Err(RepositoryError::Conflict(
                            "Application end-user identity is already in use".into(),
                        )
                        .into())
                    }
                    Some(_) => {}
                    None => insert_end_user(transaction, &write.end_user)
                        .await
                        .map_err(|error| {
                            write_error(error, "Application end-user identity is already in use")
                        })?,
                }
                insert_session(transaction, &write.session)
                    .await
                    .map_err(|error| {
                        write_error(error, "Application session identity is already in use")
                    })?;
                insert_variable_revision(transaction, &write.initial_variables)
                    .await
                    .map_err(|error| {
                        write_error(
                            error,
                            "Conversation variable revision identity is already in use",
                        )
                    })?;
                Ok(IdempotentWrite {
                    value: write.session,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn request_invocation(
    executor: &PostgresExecutor,
    write: RequestApplicationInvocationWrite,
) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
    write.validate().map_err(RepositoryError::Storage)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let current_session = lock_session(
                    transaction,
                    write.invocation.organization_id,
                    write.invocation.project_id,
                    write.invocation.application_id,
                    write.invocation.session_id,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if let Some(current) = lock_invocation(
                    transaction,
                    write.invocation.organization_id,
                    write.invocation.project_id,
                    write.invocation.application_id,
                    write.invocation.id,
                )
                .await?
                {
                    let input = load_message(transaction, &write.input_message)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Application invocation replay input message is missing".into(),
                            )
                        })?;
                    if !write
                        .matches_replay(&current, &input)
                        .map_err(PostgresPersistenceError::Invariant)?
                    {
                        return Err(RepositoryError::Conflict(
                            "Application invocation identity was reused with different input"
                                .into(),
                        )
                        .into());
                    }
                    return Ok(IdempotentWrite {
                        value: current,
                        replayed: true,
                    });
                }

                write
                    .validate_against(&current_session)
                    .map_err(RepositoryError::Conflict)?;
                let next_session = current_session
                    .append_message(write.expected_session_version, &write.input_message)
                    .map_err(RepositoryError::Conflict)?;
                insert_invocation(transaction, &write.invocation)
                    .await
                    .map_err(|error| {
                        write_error(error, "Application invocation identity is already in use")
                    })?;
                insert_message(transaction, &write.input_message)
                    .await
                    .map_err(|error| {
                        write_error(
                            error,
                            "Application message identity or sequence is already in use",
                        )
                    })?;
                update_session(transaction, &next_session, write.expected_session_version).await?;
                Ok(IdempotentWrite {
                    value: write.invocation,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}
