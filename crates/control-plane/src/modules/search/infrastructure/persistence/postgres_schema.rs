use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct AuthorizedSearchProjections => "authorized_search_projections" {
        organization_id: Uuid => "organization_id",
        project_id: Option<Uuid> => "project_id",
        environment_id: Option<Uuid> => "environment_id",
        workload_id: Option<Uuid> => "workload_id",
        resource_kind: String => "resource_kind",
        resource_id: Uuid => "resource_id",
        title: String => "title",
        description: String => "description",
        state: Option<String> => "state",
        updated_at: DateTime<Utc> => "updated_at",
        resource_id_text: String => "resource_id_text",
        title_key: String => "title_key",
        search_text: String => "search_text",
    }
}
