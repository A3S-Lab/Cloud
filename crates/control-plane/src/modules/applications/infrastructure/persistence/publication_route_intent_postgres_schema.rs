use a3s_orm::SqlArray;
use uuid::Uuid;

a3s_orm::orm_table! {
    pub(super) struct ApplicationPublicationRouteIntents => "application_publication_route_intents" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        application_id: Uuid => "application_id",
        application_release_id: Uuid => "application_release_id",
        application_release_digest: String => "application_release_digest",
        id: Uuid => "id",
        channels: SqlArray<String> => "channels",
        embed_origin_allowlist: SqlArray<String> => "embed_origin_allowlist",
        rate_shaping_profile_id: String => "rate_shaping_profile_id",
        rate_shaping_policy_revision_digest: String => "rate_shaping_policy_revision_digest",
    }
}