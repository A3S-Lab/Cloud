use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct ApplicationDeliveryCredentials => "application_delivery_credentials" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        application_id: Uuid => "application_id",
        id: Uuid => "id",
        audience: String => "audience",
        lookup_key: String => "lookup_key",
        secret_id: Uuid => "secret_id",
        secret_version: u64 => "secret_version",
        generation: u64 => "generation",
        status: String => "status",
        created_by: Uuid => "created_by",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        revoked_at: Option<DateTime<Utc>> => "revoked_at",
    }
}
