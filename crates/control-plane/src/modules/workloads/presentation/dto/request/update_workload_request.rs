use crate::modules::workloads::presentation::dto::ServiceTemplateDto;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateWorkloadRequest {
    pub template: ServiceTemplateDto,
}
