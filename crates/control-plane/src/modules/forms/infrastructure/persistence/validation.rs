use crate::modules::forms::domain::{FormDraft, FormPublicationRecord};
use crate::modules::shared_kernel::domain::{PrincipalId, RepositoryError};

pub(super) fn validate_initial_draft(
    draft: &FormDraft,
    actor_principal_id: PrincipalId,
) -> Result<(), RepositoryError> {
    draft.validate().map_err(RepositoryError::Storage)?;
    if draft.aggregate_version != 1
        || draft.latest_release.is_some()
        || draft.created_by != actor_principal_id
        || draft.updated_by != actor_principal_id
    {
        return Err(RepositoryError::Storage(
            "initial Form draft write is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_revision(
    current: &FormDraft,
    next: &FormDraft,
    expected_version: u64,
    actor_principal_id: PrincipalId,
) -> Result<(), RepositoryError> {
    next.validate().map_err(RepositoryError::Storage)?;
    if actor_principal_id != next.updated_by {
        return Err(RepositoryError::Storage(
            "Form draft revision actor does not match its aggregate".into(),
        ));
    }
    let expected = current
        .revise(
            expected_version,
            next.name.clone(),
            next.description.clone(),
            next.document.clone(),
            next.updated_by,
            next.updated_at,
        )
        .map_err(RepositoryError::Conflict)?;
    if expected != *next {
        return Err(RepositoryError::Storage(
            "Form draft revision does not match its current aggregate".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_publication_record(
    record: &FormPublicationRecord,
) -> Result<(), RepositoryError> {
    record.draft.validate().map_err(RepositoryError::Storage)?;
    record
        .release
        .validate()
        .map_err(RepositoryError::Storage)?;
    let Some(latest) = &record.draft.latest_release else {
        return Err(RepositoryError::Storage(
            "published Form draft has no latest release".into(),
        ));
    };
    if record.draft.organization_id != record.release.organization_id
        || record.draft.project_id != record.release.project_id
        || record.draft.id != record.release.form_id
        || latest.id != record.release.id
        || latest.revision != record.release.revision
        || latest.source_draft_version != record.release.source_draft_version
        || latest.digest != *record.release.content.digest()
        || latest.published_at != record.release.published_at
    {
        return Err(RepositoryError::Storage(
            "Form publication record is inconsistent".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_publication(
    current: &FormDraft,
    record: &FormPublicationRecord,
    expected_version: u64,
    actor_principal_id: PrincipalId,
) -> Result<(), RepositoryError> {
    validate_publication_record(record)?;
    if actor_principal_id != record.release.published_by
        || actor_principal_id != record.draft.updated_by
    {
        return Err(RepositoryError::Storage(
            "Form release publisher does not match its aggregate".into(),
        ));
    }
    let expected = current
        .record_release(expected_version, &record.release)
        .map_err(RepositoryError::Conflict)?;
    if expected != record.draft {
        return Err(RepositoryError::Storage(
            "Form publication aggregate does not match its source draft".into(),
        ));
    }
    Ok(())
}

pub(super) fn form_name_key(value: &str) -> String {
    value.trim().to_lowercase()
}
