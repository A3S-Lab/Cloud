use chrono::{DateTime, Utc};
use uuid::Uuid;

a3s_orm::orm_table! {
    pub(super) struct ApplicationFeedbacks => "application_feedbacks" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        application_id: Uuid => "application_id",
        application_release_id: Uuid => "application_release_id",
        application_release_digest: String => "application_release_digest",
        session_id: Uuid => "session_id",
        end_user_id: Uuid => "end_user_id",
        source_message_id: Option<Uuid> => "source_message_id",
        id: Uuid => "id",
        rating: String => "rating",
        comment: Option<String> => "comment",
        content_digest: String => "content_digest",
        created_at: DateTime<Utc> => "created_at",
    }
}
