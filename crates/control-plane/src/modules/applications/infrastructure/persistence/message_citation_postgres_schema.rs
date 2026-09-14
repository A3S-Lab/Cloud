use chrono::{DateTime, Utc};
use uuid::Uuid;

a3s_orm::orm_table! {
    pub(super) struct ApplicationMessageCitations => "application_message_citations" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        application_id: Uuid => "application_id",
        application_release_id: Uuid => "application_release_id",
        application_release_digest: String => "application_release_digest",
        session_id: Uuid => "session_id",
        end_user_id: Uuid => "end_user_id",
        invocation_id: Uuid => "invocation_id",
        message_id: Uuid => "message_id",
        message_kind: String => "message_kind",
        knowledge_base_id: Uuid => "knowledge_base_id",
        knowledge_base_revision_id: Uuid => "knowledge_base_revision_id",
        knowledge_document_id: Uuid => "knowledge_document_id",
        knowledge_chunk_id: Uuid => "knowledge_chunk_id",
        excerpt: Option<String> => "excerpt",
        excerpt_digest: String => "excerpt_digest",
        id: Uuid => "id",
        created_at: DateTime<Utc> => "created_at",
    }
}

