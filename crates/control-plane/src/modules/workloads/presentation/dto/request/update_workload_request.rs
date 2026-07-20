use super::create_workload_request::ServiceTemplateRequest;
use crate::modules::workloads::domain::entities::RequestedServiceTemplate;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateWorkloadRequest {
    pub template: ServiceTemplateRequest,
}

impl UpdateWorkloadRequest {
    pub fn into_domain(self) -> RequestedServiceTemplate {
        self.template.into_domain()
    }
}
