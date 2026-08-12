use crate::modules::workloads::presentation::dto::ServiceTemplateDto;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkloadRequest {
    pub name: String,
    #[serde(default)]
    pub node_pool_id: Option<Uuid>,
    pub template: ServiceTemplateDto,
}
