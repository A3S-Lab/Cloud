use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::applications::domain::{
    Application, ApplicationExperience, ApplicationRecord, ApplicationRelease,
    ApplicationWriteReference, APPLICATION_RELEASE_CONTRACT_SCHEMA,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    ResourceName, Sha256Digest,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresTransaction, Row, SqlQuery};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_APPLICATIONS: &str = "select organization_id, project_id, id, name, description, experience, current_release_id, current_release_number, current_release_digest, aggregate_version, created_by, created_at, updated_at from applications";
const SELECT_RELEASES: &str = "select organization_id, project_id, application_id, id, release_number, parent_release_id, parent_digest, experience, contract_schema, canonical_acl, contract_digest, workflow_definition_id, workflow_revision_id, workflow_contract_digest, workflow_payload_set_digest, workflow_semantic_contract_set_digest, input_schema_digest, output_schema_digest, presentation_digest, created_by, created_at from application_releases";

pub(super) struct ApplicationRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    name: String,
    description: String,
    experience: String,
    current_release_id: Uuid,
    current_release_number: u64,
    current_release_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for ApplicationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            name: decode(row, 3)?,
            description: decode(row, 4)?,
            experience: decode(row, 5)?,
            current_release_id: decode(row, 6)?,
            current_release_number: decode(row, 7)?,
            current_release_digest: decode(row, 8)?,
            aggregate_version: decode(row, 9)?,
            created_by: decode(row, 10)?,
            created_at: decode(row, 11)?,
            updated_at: decode(row, 12)?,
        })
    }
}

pub(super) struct ApplicationReleaseRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    id: Uuid,
    release_number: u64,
    parent_release_id: Option<Uuid>,
    parent_digest: Option<String>,
    experience: String,
    contract_schema: String,
    canonical_acl: String,
    contract_digest: String,
    workflow_definition_id: Uuid,
    workflow_revision_id: Uuid,
    workflow_contract_digest: String,
    workflow_payload_set_digest: String,
    workflow_semantic_contract_set_digest: String,
    input_schema_digest: String,
    output_schema_digest: String,
    presentation_digest: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for ApplicationReleaseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            id: decode(row, 3)?,
            release_number: decode(row, 4)?,
            parent_release_id: decode(row, 5)?,
            parent_digest: decode(row, 6)?,
            experience: decode(row, 7)?,
            contract_schema: decode(row, 8)?,
            canonical_acl: decode(row, 9)?,
            contract_digest: decode(row, 10)?,
            workflow_definition_id: decode(row, 11)?,
            workflow_revision_id: decode(row, 12)?,
            workflow_contract_digest: decode(row, 13)?,
            workflow_payload_set_digest: decode(row, 14)?,
            workflow_semantic_contract_set_digest: decode(row, 15)?,
            input_schema_digest: decode(row, 16)?,
            output_schema_digest: decode(row, 17)?,
            presentation_digest: decode(row, 18)?,
            created_by: decode(row, 19)?,
            created_at: decode(row, 20)?,
        })
    }
}

pub(super) fn application_select() -> SqlQuery<ApplicationRow> {
    sql_query::<ApplicationRow>(SELECT_APPLICATIONS)
}

pub(super) fn release_select() -> SqlQuery<ApplicationReleaseRow> {
    sql_query::<ApplicationReleaseRow>(SELECT_RELEASES)
}

pub(super) async fn lock_application(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
) -> Result<Application, PostgresPersistenceError> {
    fetch_optional::<ApplicationRow, _>(
        transaction,
        application_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and id = ")
            .bind(application_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_application)
    .transpose()?
    .ok_or_else(|| RepositoryError::NotFound.into())
}

pub(super) async fn load_record(
    transaction: &PostgresTransaction,
    reference: ApplicationWriteReference,
) -> Result<ApplicationRecord, PostgresPersistenceError> {
    let head = fetch_optional::<ApplicationRow, _>(
        transaction,
        application_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and id = ")
            .bind(reference.application_id.as_uuid()),
    )
    .await?
    .map(decode_application)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Application replay head is missing".into())
    })?;
    let release = fetch_optional::<ApplicationReleaseRow, _>(
        transaction,
        release_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and application_id = ")
            .bind(reference.application_id.as_uuid())
            .append(" and id = ")
            .bind(reference.release_id.as_uuid()),
    )
    .await?
    .map(decode_release)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Application replay release is missing".into())
    })?;
    let application = head.at_release(&release).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "Application replay target is invalid: {error}"
        ))
    })?;
    ApplicationRecord::new(application, release).map_err(PostgresPersistenceError::Invariant)
}

pub(super) fn decode_application(row: ApplicationRow) -> Result<Application, RepositoryError> {
    let application = Application {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        id: ApplicationId::from_uuid(row.id),
        name: ResourceName::parse(row.name).map_err(stored("Application name"))?,
        description: row.description,
        experience: ApplicationExperience::parse(&row.experience)
            .map_err(stored("Application experience"))?,
        current_release_id: ApplicationReleaseId::from_uuid(row.current_release_id),
        current_release_number: row.current_release_number,
        current_release_digest: Sha256Digest::parse(row.current_release_digest)
            .map_err(stored("Application release digest"))?,
        aggregate_version: row.aggregate_version,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    application
        .validate()
        .map_err(stored("Application state"))?;
    Ok(application)
}

pub(super) fn decode_release(
    row: ApplicationReleaseRow,
) -> Result<ApplicationRelease, RepositoryError> {
    let parent_digest = row
        .parent_digest
        .map(Sha256Digest::parse)
        .transpose()
        .map_err(stored("Application parent digest"))?;
    let release = ApplicationRelease::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        ApplicationId::from_uuid(row.application_id),
        ApplicationReleaseId::from_uuid(row.id),
        row.release_number,
        row.parent_release_id.map(ApplicationReleaseId::from_uuid),
        parent_digest,
        &row.canonical_acl,
        &row.contract_digest,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(stored("Application release"))?;
    let spec = release.contract.spec();
    if row.contract_schema != APPLICATION_RELEASE_CONTRACT_SCHEMA
        || row.experience != spec.experience.as_str()
        || row.workflow_definition_id != spec.workflow.workflow_definition_id.as_uuid()
        || row.workflow_revision_id != spec.workflow.workflow_revision_id.as_uuid()
        || row.workflow_contract_digest != spec.workflow.workflow_contract_digest.as_str()
        || row.workflow_payload_set_digest != spec.workflow.workflow_payload_set_digest.as_str()
        || row.workflow_semantic_contract_set_digest
            != spec.workflow.workflow_semantic_contract_set_digest.as_str()
        || row.input_schema_digest != spec.workflow.input_schema_digest.as_str()
        || row.output_schema_digest != spec.workflow.output_schema_digest.as_str()
        || row.presentation_digest != spec.presentation_digest.as_str()
    {
        return Err(RepositoryError::Storage(
            "stored Application release metadata does not match its canonical ACL".into(),
        ));
    }
    Ok(release)
}

pub(super) fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}
