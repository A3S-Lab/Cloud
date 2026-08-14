use crate::modules::workflow::application::{WorkflowPayloadAcl, WorkflowSemanticContractAcls};
use crate::modules::workflow::domain::WorkflowPayloadKind;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishWorkflowDefinitionRequest {
    pub definition_acl: String,
    pub payloads: Vec<WorkflowPayloadAclRequest>,
    #[serde(default)]
    pub semantic_contracts: Option<WorkflowSemanticContractAclsRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowSemanticContractAclsRequest {
    pub descriptor_bindings_acl: String,
    pub descriptor_registry_acl: String,
    pub variable_contract_acl: String,
    #[serde(default)]
    pub variable_defaults_acl: Option<String>,
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
    pub fn into_parts(
        self,
    ) -> (
        String,
        Vec<WorkflowPayloadAcl>,
        Option<WorkflowSemanticContractAcls>,
    ) {
        (
            self.definition_acl,
            self.payloads
                .into_iter()
                .map(WorkflowPayloadAcl::from)
                .collect(),
            self.semantic_contracts
                .map(|value| WorkflowSemanticContractAcls {
                    descriptor_bindings_acl: value.descriptor_bindings_acl,
                    descriptor_registry_acl: value.descriptor_registry_acl,
                    variable_contract_acl: value.variable_contract_acl,
                    variable_defaults_acl: value.variable_defaults_acl,
                }),
        )
    }
}
