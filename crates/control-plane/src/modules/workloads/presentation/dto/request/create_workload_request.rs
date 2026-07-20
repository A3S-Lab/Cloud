use crate::modules::workloads::presentation::dto::ServiceTemplateDto;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkloadRequest {
    pub name: String,
    pub template: ServiceTemplateDto,
}
