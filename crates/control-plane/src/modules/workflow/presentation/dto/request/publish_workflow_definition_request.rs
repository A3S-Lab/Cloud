use crate::modules::workflow::application::WorkflowPayloadAcl;
use crate::modules::workflow::domain::WorkflowPayloadKind;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishWorkflowDefinitionRequest {
    pub definition_acl: String,
    pub payloads: Vec<WorkflowPayloadAclRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowPayloadAclRequest {
    pub kind: WorkflowPayloadKind,
    pub acl: String,
}

impl From<WorkflowPayloadAclRequest> for WorkflowPayloadAcl {
    fn from(value: WorkflowPayloadAclRequest) -> Self {
        Self {
            kind: value.kind,
            acl: value.acl,
        }
    }
}

impl PublishWorkflowDefinitionRequest {
    pub fn into_parts(self) -> (String, Vec<WorkflowPayloadAcl>) {
        (
            self.definition_acl,
            self.payloads
                .into_iter()
                .map(WorkflowPayloadAcl::from)
                .collect(),
        )
    }
}
