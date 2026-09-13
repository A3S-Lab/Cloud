use super::catalog::{KnowledgeBaseRecord, KnowledgePipelineRecord};
use super::chunk::KnowledgeChunkV1;
use super::document::KnowledgeDocumentV1;
use super::document_catalog::{KnowledgeChunkRecord, KnowledgeDocumentRecord};
use crate::modules::shared_kernel::domain::{
    validate_audit_action, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
};
use a3s_cloud_contracts::{CloudScopeRef, DomainEventEnvelope};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const KNOWLEDGE_BASE_LIFECYCLE_EVENT_SCHEMA: &str = "cloud.knowledge-base.lifecycle.v1";
pub const KNOWLEDGE_PIPELINE_LIFECYCLE_EVENT_SCHEMA: &str = "cloud.knowledge-pipeline.lifecycle.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeBaseLifecycleChanged {
    pub project_id: Uuid,
    pub knowledge_base_id: Uuid,
    pub revision_id: Uuid,
    pub generation: u64,
    pub revision_digest: String,
    pub action: String,
}

impl KnowledgeBaseLifecycleChanged {
    pub fn created(
        record: &KnowledgeBaseRecord,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(record, "created", "knowledge.base.created", request_id)
    }

    pub fn revised(
        record: &KnowledgeBaseRecord,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(record, "revised", "knowledge.base.revised", request_id)
    }

    fn envelope(
        record: &KnowledgeBaseRecord,
        action: &str,
        event_key: &str,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let spec = record.revision.spec();
        let payload = Self {
            project_id: spec.project_id.as_uuid(),
            knowledge_base_id: spec.knowledge_base_id.as_uuid(),
            revision_id: spec.revision_id.as_uuid(),
            generation: spec.generation,
            revision_digest: record.revision.digest().as_str().to_string(),
            action: action.to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: CloudScopeRef::Organization {
                organization_id: spec.organization_id.as_uuid(),
            },
            aggregate_id: spec.knowledge_base_id.as_uuid(),
            aggregate_version: spec.generation,
            occurred_at: record.updated_at,
            correlation_id: request_id,
            causation_id: None,
            payload: serde_json::to_value(payload).map_err(|error| error.to_string())?,
        })
    }

    pub fn matches(&self, record: &KnowledgeBaseRecord, action: &str) -> bool {
        let spec = record.revision.spec();
        self.project_id == spec.project_id.as_uuid()
            && self.knowledge_base_id == spec.knowledge_base_id.as_uuid()
            && self.revision_id == spec.revision_id.as_uuid()
            && self.generation == spec.generation
            && self.revision_digest == record.revision.digest().as_str()
            && self.action == action
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgePipelineLifecycleChanged {
    pub project_id: Uuid,
    pub pipeline_id: Uuid,
    pub release_id: Uuid,
    pub release_digest: String,
    pub action: String,
}

impl KnowledgePipelineLifecycleChanged {
    pub fn created(
        record: &KnowledgePipelineRecord,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(record, "created", "knowledge.pipeline.created", request_id)
    }

    pub fn published(
        record: &KnowledgePipelineRecord,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(
            record,
            "published",
            "knowledge.pipeline.published",
            request_id,
        )
    }

    fn envelope(
        record: &KnowledgePipelineRecord,
        action: &str,
        event_key: &str,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let spec = record.release.spec();
        let payload = Self {
            project_id: spec.project_id.as_uuid(),
            pipeline_id: spec.pipeline_id.as_uuid(),
            release_id: spec.release_id.as_uuid(),
            release_digest: record.release.digest().as_str().to_string(),
            action: action.to_owned(),
        };
        // Aggregate version is release ordinal unknown; use 1 for create and bump via updated_at fence only.
        let aggregate_version = if action == "created" { 1 } else { 2 };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: CloudScopeRef::Organization {
                organization_id: spec.organization_id.as_uuid(),
            },
            aggregate_id: spec.pipeline_id.as_uuid(),
            aggregate_version,
            occurred_at: record.updated_at,
            correlation_id: request_id,
            causation_id: None,
            payload: serde_json::to_value(payload).map_err(|error| error.to_string())?,
        })
    }

    pub fn matches(&self, record: &KnowledgePipelineRecord, action: &str) -> bool {
        let spec = record.release.spec();
        self.project_id == spec.project_id.as_uuid()
            && self.pipeline_id == spec.pipeline_id.as_uuid()
            && self.release_id == spec.release_id.as_uuid()
            && self.release_digest == record.release.digest().as_str()
            && self.action == action
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeBaseWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub knowledge_base_id: Uuid,
    pub revision_id: Uuid,
    pub generation: u64,
}

impl From<&KnowledgeBaseRecord> for KnowledgeBaseWriteReference {
    fn from(record: &KnowledgeBaseRecord) -> Self {
        let spec = record.revision.spec();
        Self {
            organization_id: spec.organization_id,
            project_id: spec.project_id,
            knowledge_base_id: spec.knowledge_base_id.as_uuid(),
            revision_id: spec.revision_id.as_uuid(),
            generation: spec.generation,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgeBaseWrite {
    pub record: KnowledgeBaseRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateKnowledgeBaseWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.revision.validate()?;
        if self.record.revision.spec().generation != 1 {
            return Err("KnowledgeBase create write requires generation 1".into());
        }
        validate_audit_action("knowledge.base.created")?;
        validate_knowledge_base_event(&self.event, &self.record, self.request_id, "created")
    }
}

#[derive(Debug, Clone)]
pub struct AppendKnowledgeBaseWrite {
    pub record: KnowledgeBaseRecord,
    pub expected_revision_digest: String,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl AppendKnowledgeBaseWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.revision.validate()?;
        if self.record.revision.spec().generation < 2 {
            return Err("KnowledgeBase append write requires generation >= 2".into());
        }
        validate_audit_action("knowledge.base.revised")?;
        validate_knowledge_base_event(&self.event, &self.record, self.request_id, "revised")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePipelineWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub pipeline_id: Uuid,
    pub release_id: Uuid,
    pub release_digest: String,
}

impl From<&KnowledgePipelineRecord> for KnowledgePipelineWriteReference {
    fn from(record: &KnowledgePipelineRecord) -> Self {
        let spec = record.release.spec();
        Self {
            organization_id: spec.organization_id,
            project_id: spec.project_id,
            pipeline_id: spec.pipeline_id.as_uuid(),
            release_id: spec.release_id.as_uuid(),
            release_digest: record.release.digest().as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgePipelineWrite {
    pub record: KnowledgePipelineRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateKnowledgePipelineWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.release.validate()?;
        validate_audit_action("knowledge.pipeline.created")?;
        validate_knowledge_pipeline_event(&self.event, &self.record, self.request_id, "created")
    }
}

#[derive(Debug, Clone)]
pub struct PublishKnowledgePipelineWrite {
    pub record: KnowledgePipelineRecord,
    pub expected_release_digest: String,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl PublishKnowledgePipelineWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.release.validate()?;
        validate_audit_action("knowledge.pipeline.published")?;
        validate_knowledge_pipeline_event(&self.event, &self.record, self.request_id, "published")
    }
}

fn validate_knowledge_base_event(
    event: &DomainEventEnvelope,
    record: &KnowledgeBaseRecord,
    request_id: Uuid,
    action: &str,
) -> Result<(), String> {
    event.validate()?;
    let expected_key = match action {
        "created" => "knowledge.base.created",
        "revised" => "knowledge.base.revised",
        _ => return Err("unknown KnowledgeBase lifecycle action".into()),
    };
    if event.event_key != expected_key
        || event.correlation_id != request_id
        || event.aggregate_id != record.revision.spec().knowledge_base_id.as_uuid()
        || event.aggregate_version != record.revision.spec().generation
        || event.occurred_at != record.updated_at
    {
        return Err("KnowledgeBase lifecycle event drifted from record".into());
    }
    let payload: KnowledgeBaseLifecycleChanged =
        serde_json::from_value(event.payload.clone()).map_err(|error| error.to_string())?;
    if !payload.matches(record, action) {
        return Err("KnowledgeBase lifecycle payload drifted from record".into());
    }
    let _ = KNOWLEDGE_BASE_LIFECYCLE_EVENT_SCHEMA;
    Ok(())
}

fn validate_knowledge_pipeline_event(
    event: &DomainEventEnvelope,
    record: &KnowledgePipelineRecord,
    request_id: Uuid,
    action: &str,
) -> Result<(), String> {
    event.validate()?;
    let expected_key = match action {
        "created" => "knowledge.pipeline.created",
        "published" => "knowledge.pipeline.published",
        _ => return Err("unknown KnowledgePipeline lifecycle action".into()),
    };
    if event.event_key != expected_key
        || event.correlation_id != request_id
        || event.aggregate_id != record.release.spec().pipeline_id.as_uuid()
        || event.occurred_at != record.updated_at
    {
        return Err("KnowledgePipeline lifecycle event drifted from record".into());
    }
    let payload: KnowledgePipelineLifecycleChanged =
        serde_json::from_value(event.payload.clone()).map_err(|error| error.to_string())?;
    if !payload.matches(record, action) {
        return Err("KnowledgePipeline lifecycle payload drifted from record".into());
    }
    let _ = KNOWLEDGE_PIPELINE_LIFECYCLE_EVENT_SCHEMA;
    Ok(())
}

pub const KNOWLEDGE_DOCUMENT_LIFECYCLE_EVENT_SCHEMA: &str = "cloud.knowledge-document.lifecycle.v1";
pub const KNOWLEDGE_CHUNK_LIFECYCLE_EVENT_SCHEMA: &str = "cloud.knowledge-chunk.lifecycle.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeDocumentLifecycleChanged {
    pub project_id: Uuid,
    pub knowledge_base_id: Uuid,
    pub document_id: Uuid,
    pub document_digest: String,
    pub action: String,
}

impl KnowledgeDocumentLifecycleChanged {
    pub fn created(
        record: &KnowledgeDocumentRecord,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(record, "created", "knowledge.document.created", request_id)
    }

    fn envelope(
        record: &KnowledgeDocumentRecord,
        action: &str,
        event_key: &str,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let spec = record.document.spec();
        let payload = Self {
            project_id: spec.project_id.as_uuid(),
            knowledge_base_id: spec.knowledge_base_id.as_uuid(),
            document_id: spec.document_id.as_uuid(),
            document_digest: record.document.digest().as_str().to_string(),
            action: action.to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: CloudScopeRef::Organization {
                organization_id: spec.organization_id.as_uuid(),
            },
            aggregate_id: spec.document_id.as_uuid(),
            aggregate_version: 1,
            occurred_at: record.created_at,
            correlation_id: request_id,
            causation_id: None,
            payload: serde_json::to_value(payload).map_err(|error| error.to_string())?,
        })
    }

    pub fn matches(&self, record: &KnowledgeDocumentRecord, action: &str) -> bool {
        let spec = record.document.spec();
        self.project_id == spec.project_id.as_uuid()
            && self.knowledge_base_id == spec.knowledge_base_id.as_uuid()
            && self.document_id == spec.document_id.as_uuid()
            && self.document_digest == record.document.digest().as_str()
            && self.action == action
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeChunkLifecycleChanged {
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub chunk_id: Uuid,
    pub chunk_digest: String,
    pub action: String,
}

impl KnowledgeChunkLifecycleChanged {
    pub fn created(
        record: &KnowledgeChunkRecord,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(record, "created", "knowledge.chunk.created", request_id)
    }

    fn envelope(
        record: &KnowledgeChunkRecord,
        action: &str,
        event_key: &str,
        request_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let spec = record.chunk.spec();
        let payload = Self {
            project_id: spec.project_id.as_uuid(),
            document_id: spec.document_id.as_uuid(),
            chunk_id: spec.chunk_id.as_uuid(),
            chunk_digest: record.chunk.digest().as_str().to_string(),
            action: action.to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: CloudScopeRef::Organization {
                organization_id: spec.organization_id.as_uuid(),
            },
            aggregate_id: spec.chunk_id.as_uuid(),
            aggregate_version: 1,
            occurred_at: record.created_at,
            correlation_id: request_id,
            causation_id: None,
            payload: serde_json::to_value(payload).map_err(|error| error.to_string())?,
        })
    }

    pub fn matches(&self, record: &KnowledgeChunkRecord, action: &str) -> bool {
        let spec = record.chunk.spec();
        self.project_id == spec.project_id.as_uuid()
            && self.document_id == spec.document_id.as_uuid()
            && self.chunk_id == spec.chunk_id.as_uuid()
            && self.chunk_digest == record.chunk.digest().as_str()
            && self.action == action
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDocumentWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub document_id: Uuid,
    pub document_digest: String,
}

impl From<&KnowledgeDocumentRecord> for KnowledgeDocumentWriteReference {
    fn from(record: &KnowledgeDocumentRecord) -> Self {
        let spec = record.document.spec();
        Self {
            organization_id: spec.organization_id,
            project_id: spec.project_id,
            document_id: spec.document_id.as_uuid(),
            document_digest: record.document.digest().as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgeDocumentWrite {
    pub record: KnowledgeDocumentRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateKnowledgeDocumentWrite {
    pub fn validate(&self) -> Result<(), String> {
        KnowledgeDocumentV1::restore(
            self.record.document.canonical_acl(),
            self.record.document.digest().as_str(),
        )?;
        validate_audit_action("knowledge.document.created")?;
        validate_knowledge_document_event(&self.event, &self.record, self.request_id, "created")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeChunkWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub chunk_id: Uuid,
    pub chunk_digest: String,
}

impl From<&KnowledgeChunkRecord> for KnowledgeChunkWriteReference {
    fn from(record: &KnowledgeChunkRecord) -> Self {
        let spec = record.chunk.spec();
        Self {
            organization_id: spec.organization_id,
            project_id: spec.project_id,
            chunk_id: spec.chunk_id.as_uuid(),
            chunk_digest: record.chunk.digest().as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgeChunkWrite {
    pub record: KnowledgeChunkRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateKnowledgeChunkWrite {
    pub fn validate(&self) -> Result<(), String> {
        KnowledgeChunkV1::restore(
            self.record.chunk.canonical_acl(),
            self.record.chunk.digest().as_str(),
        )?;
        validate_audit_action("knowledge.chunk.created")?;
        validate_knowledge_chunk_event(&self.event, &self.record, self.request_id, "created")
    }
}

fn validate_knowledge_document_event(
    event: &DomainEventEnvelope,
    record: &KnowledgeDocumentRecord,
    request_id: Uuid,
    action: &str,
) -> Result<(), String> {
    event.validate()?;
    let expected_key = match action {
        "created" => "knowledge.document.created",
        _ => return Err("unknown KnowledgeDocument lifecycle action".into()),
    };
    let spec = record.document.spec();
    if event.event_key != expected_key
        || event.correlation_id != request_id
        || event.aggregate_id != spec.document_id.as_uuid()
        || event.aggregate_version != 1
        || event.occurred_at != record.created_at
    {
        return Err("KnowledgeDocument lifecycle event drifted from record".into());
    }
    let payload: KnowledgeDocumentLifecycleChanged =
        serde_json::from_value(event.payload.clone()).map_err(|error| error.to_string())?;
    if !payload.matches(record, action) {
        return Err("KnowledgeDocument lifecycle payload drifted from record".into());
    }
    let _ = KNOWLEDGE_DOCUMENT_LIFECYCLE_EVENT_SCHEMA;
    Ok(())
}

fn validate_knowledge_chunk_event(
    event: &DomainEventEnvelope,
    record: &KnowledgeChunkRecord,
    request_id: Uuid,
    action: &str,
) -> Result<(), String> {
    event.validate()?;
    let expected_key = match action {
        "created" => "knowledge.chunk.created",
        _ => return Err("unknown KnowledgeChunk lifecycle action".into()),
    };
    let spec = record.chunk.spec();
    if event.event_key != expected_key
        || event.correlation_id != request_id
        || event.aggregate_id != spec.chunk_id.as_uuid()
        || event.aggregate_version != 1
        || event.occurred_at != record.created_at
    {
        return Err("KnowledgeChunk lifecycle event drifted from record".into());
    }
    let payload: KnowledgeChunkLifecycleChanged =
        serde_json::from_value(event.payload.clone()).map_err(|error| error.to_string())?;
    if !payload.matches(record, action) {
        return Err("KnowledgeChunk lifecycle payload drifted from record".into());
    }
    let _ = KNOWLEDGE_CHUNK_LIFECYCLE_EVENT_SCHEMA;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn knowledge_timestamp(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("timestamp")
}

#[cfg(test)]
mod tests {
    use super::super::KnowledgeBaseRevisionV1;
    use super::*;

    const BASE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-base-revision.acl"
    ));

    #[test]
    fn lifecycle_event_matches_created_record() {
        let revision = KnowledgeBaseRevisionV1::parse_acl(BASE).expect("revision");
        let created_at = DateTime::from_timestamp(1_000, 0).expect("ts");
        let record = KnowledgeBaseRecord::new(revision, created_at).expect("record");
        let request_id = Uuid::from_u128(0x55);
        let event = KnowledgeBaseLifecycleChanged::created(&record, request_id).expect("event");
        let write = CreateKnowledgeBaseWrite {
            record: record.clone(),
            event,
            actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x66)),
            request_id,
            idempotency: IdempotencyRequest::new(
                "organizations/x/projects/y/knowledge-bases",
                "key-1",
                b"canonical",
            )
            .expect("idempotency"),
        };
        write.validate().expect("valid");
    }
}
