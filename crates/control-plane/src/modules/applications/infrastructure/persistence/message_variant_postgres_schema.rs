use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

a3s_orm::orm_table! {
    pub(super) struct ApplicationMessageVariants => "application_message_variants" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        application_id: Uuid => "application_id",
        application_release_id: Uuid => "application_release_id",
        application_release_digest: String => "application_release_digest",
        session_id: Uuid => "session_id",
        end_user_id: Uuid => "end_user_id",
        invocation_id: Uuid => "invocation_id",
        source_message_id: Uuid => "source_message_id",
        source_message_kind: String => "source_message_kind",
        id: Uuid => "id",
        instruction: Option<Value> => "instruction",
        instruction_digest: String => "instruction_digest",
        created_at: DateTime<Utc> => "created_at",
    }
}
