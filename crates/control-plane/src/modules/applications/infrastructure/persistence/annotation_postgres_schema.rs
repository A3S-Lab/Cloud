use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

a3s_orm::orm_table! {
    pub(super) struct ApplicationAnnotations => "application_annotations" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        application_id: Uuid => "application_id",
        application_release_id: Uuid => "application_release_id",
        application_release_digest: String => "application_release_digest",
        session_id: Uuid => "session_id",
        end_user_id: Uuid => "end_user_id",
        source_message_id: Option<Uuid> => "source_message_id",
        id: Uuid => "id",
        content: Value => "content",
        content_digest: String => "content_digest",
        created_at: DateTime<Utc> => "created_at",
    }
}
