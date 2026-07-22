use crate::modules::workloads::presentation::dto::SourceWorkloadTemplateDto;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSourceWorkloadRequest {
    pub name: String,
    pub template: SourceWorkloadTemplateDto,
}
