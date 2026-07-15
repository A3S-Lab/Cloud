use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishRouteRequest {
    pub workload_revision_id: Uuid,
    pub hostname: String,
    pub path_prefix: String,
    pub port_name: String,
}
