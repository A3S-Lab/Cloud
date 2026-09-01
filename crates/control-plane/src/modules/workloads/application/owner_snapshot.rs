use crate::modules::shared_kernel::domain::RepositoryError;

pub(super) fn require_unchanged_owner_snapshot<T>(
    projection: &str,
    expected: &T,
    current: &T,
) -> Result<(), RepositoryError>
where
    T: PartialEq + ?Sized,
{
    if current != expected {
        return Err(owner_snapshot_changed(projection));
    }
    Ok(())
}

pub(super) fn concurrent_owner_projection_error(
    projection: &str,
    label: &str,
    error: RepositoryError,
) -> RepositoryError {
    match error {
        RepositoryError::NotFound | RepositoryError::Conflict(_) => {
            RepositoryError::Conflict(format!("Workloads {label} changed during {projection}"))
        }
        error => error,
    }
}

pub(super) fn owner_snapshot_changed(projection: &str) -> RepositoryError {
    RepositoryError::Conflict(format!("Workloads owner state changed during {projection}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_owner_snapshot_is_the_only_accepted_snapshot() {
        assert!(require_unchanged_owner_snapshot("test projection", &(1, 2), &(1, 2)).is_ok());
        assert_eq!(
            require_unchanged_owner_snapshot("test projection", &(1, 2), &(1, 3)),
            Err(RepositoryError::Conflict(
                "Workloads owner state changed during test projection".into()
            ))
        );
    }

    #[test]
    fn concurrent_owner_reads_fail_closed_without_hiding_storage_failures() {
        assert_eq!(
            concurrent_owner_projection_error(
                "test projection",
                "record",
                RepositoryError::NotFound,
            ),
            RepositoryError::Conflict("Workloads record changed during test projection".into())
        );
        assert_eq!(
            concurrent_owner_projection_error(
                "test projection",
                "record",
                RepositoryError::Storage("offline".into()),
            ),
            RepositoryError::Storage("offline".into())
        );
    }
}
