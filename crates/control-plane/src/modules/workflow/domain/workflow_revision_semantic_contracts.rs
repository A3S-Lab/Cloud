use super::{
    WorkflowSpec, WorkflowStepBindingKind, WorkflowStepDescriptorBindings,
    WorkflowStepDescriptorRegistry, WorkflowStepOwner, WorkflowVariableContract,
    WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA, WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA,
    WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION, WORKFLOW_VARIABLE_CONTRACT_SCHEMA,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkflowRevisionSemanticContractKind {
    DescriptorBindings,
    DescriptorRegistry,
    VariableContract,
}

impl WorkflowRevisionSemanticContractKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DescriptorBindings => "descriptor_bindings",
            Self::DescriptorRegistry => "descriptor_registry",
            Self::VariableContract => "variable_contract",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "descriptor_bindings" => Ok(Self::DescriptorBindings),
            "descriptor_registry" => Ok(Self::DescriptorRegistry),
            "variable_contract" => Ok(Self::VariableContract),
            _ => Err(format!(
                "unsupported Workflow revision semantic contract kind {value:?}"
            )),
        }
    }
}

/// Immutable compiler inputs owned by one Workflow revision.
///
/// The registry is retained so every bound semantic digest is recoverable. Its
/// presentation and admission metadata are deliberately excluded from
/// `digest`, which is derived from the binding and variable contracts only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRevisionSemanticContracts {
    descriptor_bindings: WorkflowStepDescriptorBindings,
    descriptor_registry: WorkflowStepDescriptorRegistry,
    variable_contract: WorkflowVariableContract,
    digest: Sha256Digest,
}

impl WorkflowRevisionSemanticContracts {
    pub fn create(
        workflow: &WorkflowSpec,
        descriptor_bindings: WorkflowStepDescriptorBindings,
        descriptor_registry: WorkflowStepDescriptorRegistry,
        variable_contract: WorkflowVariableContract,
    ) -> Result<Self, String> {
        let digest = digest_contract_set(&descriptor_bindings, &variable_contract)?;
        let value = Self {
            descriptor_bindings,
            descriptor_registry,
            variable_contract,
            digest,
        };
        value.validate(workflow)?;
        Ok(value)
    }

    pub fn restore(
        workflow: &WorkflowSpec,
        descriptor_bindings_acl: &str,
        descriptor_bindings_digest: &str,
        descriptor_registry_acl: &str,
        descriptor_registry_digest: &str,
        variable_contract_acl: &str,
        variable_contract_digest: &str,
    ) -> Result<Self, String> {
        Self::create(
            workflow,
            WorkflowStepDescriptorBindings::restore(
                descriptor_bindings_acl,
                descriptor_bindings_digest,
            )?,
            WorkflowStepDescriptorRegistry::restore(
                descriptor_registry_acl,
                descriptor_registry_digest,
            )?,
            WorkflowVariableContract::restore(variable_contract_acl, variable_contract_digest)?,
        )
    }

    pub fn validate(&self, workflow: &WorkflowSpec) -> Result<(), String> {
        if self.descriptor_bindings.compiler_schema_version()
            != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
            || self.descriptor_registry.compiler_schema_version()
                != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
            || self.variable_contract.compiler_schema_version()
                != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
            || digest_contract_set(&self.descriptor_bindings, &self.variable_contract)?
                != self.digest
        {
            return Err("Workflow semantic contract compiler authority is invalid".into());
        }
        workflow.topological_order(Default::default())?;
        if self.descriptor_bindings.bindings().len() != workflow.steps.len() {
            return Err(
                "Workflow descriptor bindings must cover every graph step exactly once".into(),
            );
        }

        let mut referenced_descriptors = BTreeSet::new();
        let mut application_ports = BTreeSet::new();
        let mut descriptors_by_step = BTreeMap::new();
        for step in &workflow.steps {
            let binding = self
                .descriptor_bindings
                .resolve(&step.id)
                .ok_or_else(|| format!("Workflow step {:?} has no descriptor binding", step.id))?;
            let descriptor = self.descriptor_registry.resolve_for_compiler(
                &binding.descriptor_id,
                &binding.descriptor_revision,
                WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            )?;
            if descriptor.semantic_digest() != &binding.semantic_digest
                || descriptor.spec().kind != Some(step.kind)
            {
                return Err(format!(
                    "Workflow step {:?} descriptor semantics do not match its graph kind",
                    step.id
                ));
            }
            validate_supported_bindings(step, descriptor.spec())?;
            validate_capability_binding(step, descriptor.spec())?;
            descriptors_by_step.insert(step.id.as_str(), descriptor.spec());
            referenced_descriptors.insert((descriptor.id(), descriptor.revision()));
            if descriptor.spec().owner == WorkflowStepOwner::Applications {
                application_ports.insert(step.id.as_str());
            }
        }
        let stored_descriptors = self
            .descriptor_registry
            .descriptors()
            .iter()
            .map(|descriptor| (descriptor.id(), descriptor.revision()))
            .collect::<BTreeSet<_>>();
        if referenced_descriptors != stored_descriptors {
            return Err(
                "Workflow descriptor registry snapshot must contain exactly the bound revisions"
                    .into(),
            );
        }
        validate_variable_read_ports(self.variable_contract.spec(), &descriptors_by_step)?;
        self.variable_contract
            .validate_graph_bindings_with_application_ports(workflow, &application_ports)
    }

    pub const fn descriptor_bindings(&self) -> &WorkflowStepDescriptorBindings {
        &self.descriptor_bindings
    }

    pub const fn descriptor_registry(&self) -> &WorkflowStepDescriptorRegistry {
        &self.descriptor_registry
    }

    pub const fn variable_contract(&self) -> &WorkflowVariableContract {
        &self.variable_contract
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn schema(&self, kind: WorkflowRevisionSemanticContractKind) -> &'static str {
        match kind {
            WorkflowRevisionSemanticContractKind::DescriptorBindings => {
                WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA
            }
            WorkflowRevisionSemanticContractKind::DescriptorRegistry => {
                WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA
            }
            WorkflowRevisionSemanticContractKind::VariableContract => {
                WORKFLOW_VARIABLE_CONTRACT_SCHEMA
            }
        }
    }

    pub fn canonical_acl(&self, kind: WorkflowRevisionSemanticContractKind) -> &str {
        match kind {
            WorkflowRevisionSemanticContractKind::DescriptorBindings => {
                self.descriptor_bindings.canonical_acl()
            }
            WorkflowRevisionSemanticContractKind::DescriptorRegistry => {
                self.descriptor_registry.canonical_acl()
            }
            WorkflowRevisionSemanticContractKind::VariableContract => {
                self.variable_contract.canonical_acl()
            }
        }
    }

    pub fn contract_digest(&self, kind: WorkflowRevisionSemanticContractKind) -> &Sha256Digest {
        match kind {
            WorkflowRevisionSemanticContractKind::DescriptorBindings => {
                self.descriptor_bindings.digest()
            }
            WorkflowRevisionSemanticContractKind::DescriptorRegistry => {
                self.descriptor_registry.digest()
            }
            WorkflowRevisionSemanticContractKind::VariableContract => {
                self.variable_contract.digest()
            }
        }
    }

    pub fn requires_binding(&self, kind: WorkflowStepBindingKind) -> bool {
        self.descriptor_bindings.bindings().iter().any(|binding| {
            self.descriptor_registry
                .resolve(&binding.descriptor_id, &binding.descriptor_revision)
                .is_some_and(|descriptor| descriptor.spec().required_bindings.contains(&kind))
        })
    }
}

fn validate_variable_read_ports(
    variables: &super::WorkflowVariableContractSpec,
    descriptors: &BTreeMap<&str, &super::WorkflowStepDescriptorSpec>,
) -> Result<(), String> {
    for read in &variables.reads {
        let descriptor = descriptors
            .get(read.consumer_step_id.as_str())
            .ok_or_else(|| {
                format!(
                    "Workflow variable read {:?} has no consumer descriptor",
                    read.id
                )
            })?;
        let port = descriptor
            .input_ports
            .iter()
            .find(|port| port.name == read.target_port)
            .ok_or_else(|| {
                format!(
                    "Workflow variable read {:?} targets undeclared descriptor input {:?}",
                    read.id, read.target_port
                )
            })?;
        if port.value_type != super::WorkflowDataType::Any && port.value_type != read.expected_type
        {
            return Err(format!(
                "Workflow variable read {:?} type does not match descriptor input {:?}",
                read.id, read.target_port
            ));
        }
    }
    Ok(())
}

fn validate_supported_bindings(
    step: &super::WorkflowStepSpec,
    descriptor: &super::WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    if let Some(unsupported) = descriptor.required_bindings.iter().find(|binding| {
        !matches!(
            binding,
            WorkflowStepBindingKind::CapabilityReference | WorkflowStepBindingKind::PlacementPolicy
        )
    }) {
        return Err(format!(
            "Workflow step {:?} descriptor requires unsupported {} binding",
            step.id,
            unsupported.as_str()
        ));
    }
    Ok(())
}

fn validate_capability_binding(
    step: &super::WorkflowStepSpec,
    descriptor: &super::WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    let requires_capability = descriptor
        .required_bindings
        .contains(&WorkflowStepBindingKind::CapabilityReference);
    match (requires_capability, step.capability.as_ref()) {
        (true, Some(capability))
            if descriptor
                .allowed_capability_types
                .contains(&capability.capability_type) =>
        {
            Ok(())
        }
        (false, None) => Ok(()),
        _ => Err(format!(
            "Workflow step {:?} capability does not satisfy its descriptor",
            step.id
        )),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SemanticContractDigestInput<'a> {
    descriptor_bindings_digest: &'a str,
    variable_contract_digest: &'a str,
}

fn digest_contract_set(
    bindings: &WorkflowStepDescriptorBindings,
    variables: &WorkflowVariableContract,
) -> Result<Sha256Digest, String> {
    let encoded = serde_json::to_vec(&SemanticContractDigestInput {
        descriptor_bindings_digest: bindings.digest().as_str(),
        variable_contract_digest: variables.digest().as_str(),
    })
    .map_err(|error| format!("could not encode Workflow semantic contract set: {error}"))?;
    Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(encoded)))
}
