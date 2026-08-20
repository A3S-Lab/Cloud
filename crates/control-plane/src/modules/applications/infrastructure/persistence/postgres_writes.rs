use crate::infrastructure::{
    execute, require_one_row, store_audit, store_idempotency, store_outbox, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::applications::domain::{
    Application, ApplicationRecord, ApplicationRelease, ApplicationWriteReference,
    APPLICATION_RELEASE_CONTRACT_SCHEMA,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, PrincipalId, Sha256Digest};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{sql_query, PostgresTransaction};
use uuid::Uuid;

pub(super) async fn insert_application(
    transaction: &PostgresTransaction,
    application: &Application,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application",
        execute(
            transaction,
            sql_query::<()>("insert into applications (organization_id, project_id, id, name, name_key, description, experience, current_release_id, current_release_number, current_release_digest, aggregate_version, created_by, created_at, updated_at) values (")
                .bind(application.organization_id.as_uuid())
                .append(", ")
                .bind(application.project_id.as_uuid())
                .append(", ")
                .bind(application.id.as_uuid())
                .append(", ")
                .bind(application.name.as_str())
                .append(", ")
                .bind(application.name.key())
                .append(", ")
                .bind(application.description.as_str())
                .append(", ")
                .bind(application.experience.as_str())
                .append(", ")
                .bind(application.current_release_id.as_uuid())
                .append(", ")
                .bind(application.current_release_number)
                .append(", ")
                .bind(application.current_release_digest.as_str())
                .append(", ")
                .bind(application.aggregate_version)
                .append(", ")
                .bind(application.created_by.as_uuid())
                .append(", ")
                .bind(application.created_at)
                .append(", ")
                .bind(application.updated_at)
                .append(")"),
        )
        .await?,
    )
}

pub(super) async fn insert_release(
    transaction: &PostgresTransaction,
    release: &ApplicationRelease,
) -> Result<(), PostgresPersistenceError> {
    let spec = release.contract.spec();
    require_one_row(
        "Application release",
        execute(
            transaction,
            sql_query::<()>("insert into application_releases (organization_id, project_id, application_id, id, release_number, parent_release_id, parent_digest, experience, contract_schema, canonical_acl, contract_digest, workflow_definition_id, workflow_revision_id, workflow_contract_digest, workflow_payload_set_digest, workflow_semantic_contract_set_digest, input_schema_digest, output_schema_digest, presentation_digest, created_by, created_at) values (")
                .bind(release.organization_id.as_uuid())
                .append(", ")
                .bind(release.project_id.as_uuid())
                .append(", ")
                .bind(release.application_id.as_uuid())
                .append(", ")
                .bind(release.id.as_uuid())
                .append(", ")
                .bind(release.release_number)
                .append(", ")
                .bind(release.parent_release_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(release.parent_digest.as_ref().map(Sha256Digest::as_str))
                .append(", ")
                .bind(spec.experience.as_str())
                .append(", ")
                .bind(APPLICATION_RELEASE_CONTRACT_SCHEMA)
                .append(", ")
                .bind(release.contract.canonical_acl())
                .append(", ")
                .bind(release.contract.digest().as_str())
                .append(", ")
                .bind(spec.workflow.workflow_definition_id.as_uuid())
                .append(", ")
                .bind(spec.workflow.workflow_revision_id.as_uuid())
                .append(", ")
                .bind(spec.workflow.workflow_contract_digest.as_str())
                .append(", ")
                .bind(spec.workflow.workflow_payload_set_digest.as_str())
                .append(", ")
                .bind(spec.workflow.workflow_semantic_contract_set_digest.as_str())
                .append(", ")
                .bind(spec.workflow.input_schema_digest.as_str())
                .append(", ")
                .bind(spec.workflow.output_schema_digest.as_str())
                .append(", ")
                .bind(spec.presentation_digest.as_str())
                .append(", ")
                .bind(release.created_by.as_uuid())
                .append(", ")
                .bind(release.created_at)
                .append(")"),
        )
        .await?,
    )
}

pub(super) async fn persist_write(
    transaction: &PostgresTransaction,
    record: &ApplicationRecord,
    event: &DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(transaction, event).await?;
    store_application_audit(transaction, record, actor_principal_id, request_id).await?;
    let reference = ApplicationWriteReference::from(record);
    store_idempotency(transaction, idempotency, &reference).await
}

async fn store_application_audit(
    transaction: &PostgresTransaction,
    record: &ApplicationRecord,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.release.contract.spec();
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: record.application.organization_id.as_uuid(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action: "application.release.published",
            aggregate_id: record.application.id.as_uuid(),
            occurred_at: record.release.created_at,
            request_id,
            details: serde_json::json!({
                "projectId": record.application.project_id,
                "releaseId": record.release.id,
                "releaseNumber": record.release.release_number,
                "parentReleaseId": record.release.parent_release_id,
                "experience": spec.experience.as_str(),
                "contractSchema": APPLICATION_RELEASE_CONTRACT_SCHEMA,
                "contractDigest": record.release.contract.digest(),
                "workflowDefinitionId": spec.workflow.workflow_definition_id,
                "workflowRevisionId": spec.workflow.workflow_revision_id,
                "workflowContractDigest": spec.workflow.workflow_contract_digest,
                "workflowPayloadSetDigest": spec.workflow.workflow_payload_set_digest,
                "workflowSemanticContractSetDigest":
                    spec.workflow.workflow_semantic_contract_set_digest,
                "inputSchemaDigest": spec.workflow.input_schema_digest,
                "outputSchemaDigest": spec.workflow.output_schema_digest,
                "presentationDigest": spec.presentation_digest,
            }),
        },
    )
    .await
}
